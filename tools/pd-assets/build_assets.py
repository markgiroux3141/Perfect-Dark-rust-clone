#!/usr/bin/env python3
"""Regenerate `assets/` from the decomp, all of it, reproducibly.

    python tools/pd-assets/build_assets.py            # into assets/ (or PD_ASSETS_OUT)

Runs every exporter into the layout of docs/ARCHITECTURE.md § Assets, after
clearing the directories it owns (so nothing stale survives), then writes
`MANIFEST.json`: the decomp commit, the ROM md5, a sha256 of each exporter
script, the counts, and a digest per directory. Two runs over the same decomp
produce the same bytes; `git diff assets/` after a run is the review of an
exporter change.

| Directory     | Written by |
|---------------|------------|
| models/       | pd_models.py: guns, hands, casings, held guns, props, chr bodies and heads, the hudpiece |
| textures/     | pd_models.py: the global pool (+ models/tex/ for textures stored in a model) |
| anims/        | here: the whole animation bank, raw, as PD's bit-packed data |
| fonts/, lang/ | pd_menu_gen.py `export_assets` |
| data/         | pd_menu_gen.py (mpconfigs.bin), pd_fpgun.py `build_weapons` (weapons.json) |
| sfx/          | pd_sfx.py `export_pool` |

It also regenerates the Rust that is generated from the decomp:
`crates/pd_core/src/ids.rs` (pd_ids.py) and `crates/pd_menu/src/generated.rs`
(pd_menu_gen.py `write_rust`). Stages (`stages/<code>/`) arrive with
M3's stage exporter.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pd_fpgun  # noqa: E402
import pd_ids  # noqa: E402
import pd_menu_gen  # noqa: E402
import pd_models  # noqa: E402
import pd_sfx  # noqa: E402
from pd_paths import ASSETS, DECOMP, OUT, asset, out, require_decomp  # noqa: E402

#: The directories this script owns and clears. README.md and anything else at
#: the root are left alone.
OWNED = ["models", "textures", "anims", "fonts", "lang", "data", "sfx"]

#: md5 of the NTSC-final ROM (reference/README.md).
ROM_MD5 = "e03b088b6ac9e0080440efed07c1e40f"

#: Animations the gun code starts from C rather than from a weapon script
#: (var80070200, bondgun.c:6391: the remote mine detonator press), and the
#: head-bob / walk clips (g_HeadAnims, bondhead.c:14) plus the stand pose
#: bhead_reset measures from (bondheadreset.c:117).
CODE_ANIMS = pd_fpgun.EXTRA_ANIMS
HEAD_ANIMS = pd_fpgun.HEAD_ANIMS


def anim_flags(a: dict) -> int:
    """`ANIMFLAG_*` from the decomp's animations.json columns (LOOP 1,
    ABSOLUTETRANSLATION 2, HASREPEATFRAMES 4, 8)."""
    return (1 if a.get("flag01") else 0) | (2 if a.get("flag02") else 0) | \
           (4 if a.get("flag04") else 0) | (8 if a.get("flag08") else 0)


def export_anims() -> dict:
    """`anims/<num>.bin` (raw) + `anims/index.json` for the whole bank.

    The bank is one table indexed by `ANIM_*` number; the Combat Simulator plays
    gun scripts, the menus' ANIM_01FC/040D, and every chr row of chraction.c
    (locomotion, fire, squat/duck 0x0280-0x0287, hits, deaths), so it ships whole
    (8 MB) rather than as a reference closure that could miss a row."""
    os.makedirs(out("anims"), exist_ok=True)
    table = json.load(open(asset("animations.json"), encoding="utf-8"))
    index = {}
    for num, a in enumerate(table):
        shutil.copyfile(asset("animations", a["file"]), out("anims", f"{num:04x}.bin"))
        index[f"{num:04x}"] = {
            "id": a["id"], "numframes": a["numframes"], "bytesperframe": a["bytesperframe"],
            "headerlen": a["unk08"], "framelen": a["unk0a"], "flags": anim_flags(a),
        }
    with open(out("anims", "index.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(index, fh, indent=0)
        fh.write("\n")
    return {"anims": len(index)}


def export_weapons() -> tuple[dict, dict]:
    """`data/weapons.json`: the gset tables (pd_weapons.py + pd_fpgun.py)."""
    weapons, table = pd_fpgun.build_weapons()
    num = {a["id"]: i for i, a in enumerate(table)}
    weapons["head_anims"] = {a: num[a] for a in HEAD_ANIMS}
    weapons["code_anims"] = {a: num[a] for a in CODE_ANIMS}
    os.makedirs(out("data"), exist_ok=True)
    with open(out("data", "weapons.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(weapons, fh, indent=1)
        fh.write("\n")
    return weapons, {"weapons": len(weapons["weapons"]), "gun_scripts": len(weapons["scripts"])}


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def tree_digest(root: str) -> tuple[int, str]:
    """(file count, sha256 over sorted `relpath:sha256` lines) of a directory."""
    lines = []
    for dp, _, fs in os.walk(root):
        for f in fs:
            p = os.path.join(dp, f)
            lines.append(os.path.relpath(p, root).replace(os.sep, "/") + ":" + sha256_file(p))
    lines.sort()
    return len(lines), hashlib.sha256("\n".join(lines).encode()).hexdigest()


def decomp_commit() -> str | None:
    try:
        return subprocess.run(["git", "-C", DECOMP, "rev-parse", "HEAD"], check=True,
                              capture_output=True, text=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def rom_md5() -> str | None:
    """The md5 of the ROM the extraction came from, when it is still in the decomp."""
    rom = os.path.join(DECOMP, "pd.ntsc-final.z64")
    if not os.path.exists(rom):
        return None
    h = hashlib.md5()
    with open(rom, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    require_decomp()
    md5 = rom_md5()
    if md5 is not None and md5 != ROM_MD5:
        raise SystemExit(f"the decomp's ROM has md5 {md5}, not NTSC final ({ROM_MD5})")
    os.makedirs(OUT, exist_ok=True)
    for d in OWNED:
        shutil.rmtree(out(d), ignore_errors=True)

    counts: dict = {}
    weapons, c = export_weapons()
    counts.update(c)
    counts.update(pd_models.export_all(weapons))
    counts.update(export_anims())
    counts.update(pd_menu_gen.export_assets())
    counts.update(pd_sfx.export_pool(out("sfx")))
    pd_ids.main()
    pd_menu_gen.write_rust()

    exporters = {f: sha256_file(os.path.join(HERE, f)) for f in sorted(os.listdir(HERE)) if f.endswith(".py")}
    dirs = {}
    for d in OWNED:
        n, digest = tree_digest(out(d))
        dirs[d] = {"files": n, "sha256": digest}
    manifest = {
        "generated_by": "tools/pd-assets/build_assets.py",
        "decomp": {"repo": "https://github.com/n64decomp/perfect_dark", "commit": decomp_commit(),
                   "version": "ntsc-final", "assets": os.path.relpath(ASSETS, DECOMP).replace(os.sep, "/")},
        "rom_md5": md5 or ROM_MD5,
        "exporters": exporters,
        "counts": counts,
        "dirs": dirs,
    }
    with open(out("MANIFEST.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(manifest, fh, indent=1)
        fh.write("\n")
    print(f"build_assets: {json.dumps(counts)} -> {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
