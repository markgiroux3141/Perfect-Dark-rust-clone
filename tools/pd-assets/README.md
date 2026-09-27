# tools/pd-assets: Perfect Dark asset exporters

Copied verbatim from the old repo's `tools/pd-assets/` in M0. **They still write to the old layout (`<repo>/native/assets/...`) and must not be run until M1 repoints them** at `assets/` through a shared `pd_paths.py`, and adds `build_assets.py`.

Inputs: `reference/pd-decomp` (see [reference/README.md](../../reference/README.md), including the one-time `tools/extract`). Python 3 standard library only; `pd_bg_preview.py` also needs numpy + Pillow.

| Script | Exports | M1 plan |
|---|---|---|
| `pd_model.py`, `pd_anim.py`, `pd_pose.py`, `pd_tex.py` | shared parsers and decoders (model `.bin`, animation bank, textures) | keep; `pd_tex` also writes the global texture pool |
| `pd_fpgun.py` | guns + hands (node tree, interpreted display lists), weapon tables, gun scripts, animations | becomes the one model exporter (with `pd_menu_models`) |
| `pd_menu_models.py` | menu bodies, heads and hudpiece as `.pdm` | merged into the one model format |
| `pd_menu_gen.py` | `crates/pd_menu/src/dialogs.rs` + fonts, lang, `mpconfigs.bin`, `mpstringsE.bin` | repoint outputs |
| `pd_bg.py` | a stage's textured BG (Complex) | M3: grows into the stage exporter |
| `pd_hitbox.py` | body part boxes | merged into the model format (BBOX nodes) |
| `pd_sfx.py` | VADPCM → WAV + manifest | one `sfx/` pool |
| `pd_weapons.py` | the MP weapon table with provenance | feeds `data/weapons.json` |
| `pd_gltf.py` | skinned GLB bodies and clips for the old engine | **retired** once bodies use PD model files (reads `pd dump/`, which is not copied) |
| `pd_preview.py`, `pd_triage.py`, `pd_animmap.py`, `pd_bg_preview.py` | CPU previews and analysis | keep as tools; `pd_animmap` (GE↔PD) is unused here |
