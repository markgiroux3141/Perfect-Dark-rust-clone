# reference/: read-only third-party material (gitignored)

Nothing here is on the build path, and nothing reads it at run time. Only the asset exporters in `tools/pd-assets/` read it, offline.

| Entry | What | How to get it |
|---|---|---|
| `pd-decomp/` | Perfect Dark decomp, the spec. Cite it as `file.c:line`, NTSC final. | On this machine it is a junction to the old repo's clone. Elsewhere: `git clone --depth 1 https://github.com/n64decomp/perfect_dark reference/pd-decomp` (the spikes used commit `169ed48`) |
| `pd-pcport/` | The PC port (mouse aim, fast3d). Cited, never read by tools. | Junction here. Elsewhere: `git clone --depth 1 https://github.com/fgsfdsfgs/perfect_dark reference/pd-pcport` |
| `pd_bot_port_sheet.md` | Our notes quoting the decomp's bot code verbatim, the working sheet for the simulant port. Gitignored because it quotes the decomp. | Copied from the old repo's `reference/`. Back it up by hand. |

## One-time extraction

The decomp commits its JSON (tiles, pads, lang) and C sources, but not the binaries extracted from the ROM (models, animations, textures, fonts, sounds). The exporters need those, so on a fresh clone run:

```
cd reference/pd-decomp
ROMID=ntsc-final python tools/extract
```

It needs `pd.ntsc-final.z64` (md5 `e03b088b6ac9e0080440efed07c1e40f`) in the decomp root. The user's copy is `D:\GoldenPerfectModding\Perfect Dark (U) (V1.1) USE THIS ONE\Perfect Dark (U) (V1.1) [!].z64`.
