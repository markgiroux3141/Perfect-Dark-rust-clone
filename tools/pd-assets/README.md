# tools/pd-assets: Perfect Dark asset exporters

They read `reference/pd-decomp` (see [reference/README.md](../../reference/README.md), including the one-time `tools/extract`) and write `assets/` in the layout of [docs/ARCHITECTURE.md § Assets](../../docs/ARCHITECTURE.md#assets). Python 3 standard library only; `pd_bg_preview.py` also needs numpy + Pillow.

```
python tools/pd-assets/build_assets.py           # regenerate all of assets/ (reproducible; writes MANIFEST.json)
python tools/pd-assets/check_against_spikes.py   # compare with the old repo's per-feature exports
```

Every path comes from `pd_paths.py`: the decomp is `PD_DECOMP_DIR` or `reference/pd-decomp`, the output is `PD_ASSETS_OUT` or `assets/`. Point `PD_ASSETS_OUT` at a scratch folder to try an exporter change without touching `assets/`, then run the real build and review `git diff assets/`.

| Script | Does |
|---|---|
| `build_assets.py` | the driver: clears the directories it owns, runs everything below, exports the animation bank, writes `MANIFEST.json` |
| `pd_models.py` | the one model format (`models/<stem>.json + .bin`) for guns, hands, casings, held guns, props, chr bodies and heads, the hudpiece; the texture pool (`textures/`) |
| `pd_fpgun.py` | the display-list interpreter `export_model` that every model goes through; the weapon tables and gun scripts (`build_weapons` → `data/weapons.json`) |
| `pd_menu_gen.py` | `export_assets`: fonts, `lang/`, `data/mpconfigs.bin`. Its `main` also generates `crates/pd_menu/src/generated.rs` (M2) |
| `pd_sfx.py` | VADPCM → WAV; `export_pool` writes `sfx/` (one pool, keyed by sound number, envelope baked for non-looping sounds) |
| `pd_weapons.py` | the MP weapon table with provenance (feeds `build_weapons`) |
| `pd_model.py`, `pd_anim.py`, `pd_pose.py`, `pd_tex.py` | shared parsers and decoders (model `.bin`, animation bank, textures) |
| `pd_gltf.py` | texconfig reading and inline-texture decoding used by `export_model`. Its GLB writer is retired: bodies are PD model files now |
| `pd_stage.py` | a stage, `stages/<code>/`: the BG (`pd_bg.build`, written in the one model format, textures in the pool), `tiles.json` (collision, GEOFLAG bits), `pads.json` (pads, waypoints, waygroups, cover), `setup.json` (the MP setup's `intro[]` and `props[]`, by macro parameter name) |
| `pd_bg.py` | the BG interpreter: `bg_reset`/`bg_load_room` over `bg_<code>.seg`, every room's display lists through the GBI interpreter, the environment row (`g_NoFogEnvironments`) |
| `check_against_spikes.py` | the regression check: every texture, animation, sound, font, model, the weapon table and the stage BGs against the old repo, with the intended differences named |
| `pd_preview.py`, `pd_triage.py`, `pd_animmap.py`, `pd_bg_preview.py` | analysis tools from the spikes, kept only for reference: the first three work on the old repo's GLB exports, `pd_bg_preview.py` on the spike's inline-vertex `bg.json` (the old repo's `levels/pd_bg/`) |
