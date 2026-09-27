#!/usr/bin/env python3
"""Export the Combat Simulator menu models for the `pd_menu` spike.

The character select (setup.c:568, :1881) shows PD's own body and head models,
posed by ANIM_01FC and zooming between the full figure and the face
(menu_render_model, menu.c:1719); the "hudpiece" (the CI eye, FILE_GHUDPIECE)
unfolds with ANIM_040D in the top-left of every menu root that has one.

Every model goes through `pd_fpgun.export_model` — the display-list
interpreter the gun spike renders from, so each triangle batch carries the N64
state that drew it (combiner, lighting / texgen, tile, blender, z, cull). The
character root (`CHRINFO`) isn't described by that exporter; its rodata
(`modelrodata_chrinfo`: animpart, mtxindex) is read here.

Output (native/assets/pd_menu/models/):
  <stem>.pdm       "PDM1", u32 header length, UTF-8 JSON header (the exporter's
                   dict minus the vertex/index arrays), then per batch
                   verts (f32 x,y,z, u16 mtx, f32 u,v, u8 c0..c3, u8 flags, u8 pad)
                   and u16 indices.
  tex/tex_NNNN.png the textures, shared across models.
  index.json       {filenum: stem} for every exported file.
  anims/           the two animations (raw, as the gun spike ships them) + anims.json.

Usage:
    python tools/pd-assets/pd_menu_models.py
"""

from __future__ import annotations

import json
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pd_fpgun  # noqa: E402
import pd_model  # noqa: E402
import pd_menu_gen as gen  # noqa: E402

ROOT = gen.ROOT
FILES = os.path.join(gen.SRC, "assets", "ntsc-final", "files")
OUT = os.path.join(ROOT, "native", "assets", "pd_menu", "models")

NODE_CHRINFO = 0x01


def file_paths() -> dict[int, str]:
    """FILE number → ROM path, from list.c ("Cdark_combatZ" → chrs/dark_combat.bin)."""
    out = {}
    for m in re.finditer(r"/\*0x([0-9a-f]+)\*/\s*\"([^\"]+)\"", gen.read(os.path.join(FILES, "list.c"))):
        num, name = int(m.group(1), 16), m.group(2)
        if "/" in name:
            continue
        kind, stem = name[0], name[1:]
        if stem.endswith("Z"):
            stem = stem[:-1]
        folder = {"C": "chrs", "G": "guns", "P": "props"}.get(kind)
        if folder:
            out[num] = os.path.join(folder, stem.lower() + ".bin")
    return out


def wanted_filenums(c: gen.Consts) -> set[int]:
    mp = gen.preprocess(gen.strip_comments(gen.read(os.path.join(gen.SRC, "game", "mplayer", "mplayer.c"))))
    robot = gen.preprocess(gen.strip_comments(gen.read(os.path.join(gen.SRC, "game", "modeldata", "robot.c"))))
    hb = gen.entries(gen.find_initialisers(robot, "struct headorbody")["g_HeadsAndBodies"])
    nums = set()
    for r in gen.entries(gen.find_initialisers(mp, "struct mpbody")["g_MpBodies"]):
        nums.add(c.eval(hb[c.eval(r[0])][5]))
        head = c.eval(r[2])
        if head != 1000:
            nums.add(c.eval(hb[head][5]))
    for table in ("g_MpHeads", "g_MpBeauHeads"):
        for r in gen.entries(gen.find_initialisers(mp, "struct mphead")[table]):
            nums.add(c.eval(hb[c.eval(r[0])][5]))
    for name in ("g_MpMaleHeads", "g_MpFemaleHeads"):
        for v in gen.split_top(gen.find_initialisers(mp, "u32")[name]):
            nums.add(c.eval(hb[c.eval(v)][5]))
    nums.add(c.eval("FILE_GHUDPIECE"))
    return nums


def chrinfo(path: str) -> dict | None:
    m = pd_model.load(path)
    root = m.read_node(pd_fpgun.seg_off(m.rootnode)) if pd_fpgun.seg_ok(m.rootnode) else None
    if root is None or (root.type & 0xFF) != NODE_CHRINFO or not pd_fpgun.seg_ok(root.rodata):
        return None
    animpart, mtxindex = struct.unpack_from(">Hh", m.data, pd_fpgun.seg_off(root.rodata))
    return {"animpart": animpart, "mtx": mtxindex}


def write_pdm(d: dict, path: str) -> int:
    head = {k: v for k, v in d.items() if k != "batches"}
    head["batches"] = [{"node": b["node"], "material": b["material"], "nverts": len(b["verts"]), "nidx": len(b["indices"])} for b in d["batches"]]
    js = json.dumps(head, separators=(",", ":")).encode("utf-8")
    blob = bytearray()
    for b in d["batches"]:
        for v in b["verts"]:
            x, y, z, mtx, u, vv, c0, c1, c2, c3, flags = v
            blob += struct.pack("<fffHffBBBBBx", x, y, z, int(mtx), u, vv, int(c0), int(c1), int(c2), int(c3), int(flags))
        for i in b["indices"]:
            blob += struct.pack("<H", i)
    with open(path, "wb") as fh:
        fh.write(b"PDM1" + struct.pack("<I", len(js)) + js + bytes(blob))
    return 8 + len(js) + len(blob)


def main() -> int:
    c = gen.Consts()
    for h in ("constants.h", "files.h", "sfx.h"):
        c.load_header(os.path.join(gen.SRC, "include", h))
    for js in ("sequences.json", "animations.json"):
        rows = json.load(open(os.path.join(gen.SRC, "assets", "ntsc-final", js), encoding="utf-8"))
        for i, r in enumerate(rows):
            c.vals.setdefault(r["id"], i)

    paths = file_paths()
    nums = sorted(wanted_filenums(c))
    texdir = os.path.join(OUT, "tex")
    os.makedirs(texdir, exist_ok=True)
    index = {}
    total = 0
    for n in nums:
        rel = paths.get(n)
        if rel is None:
            print(f"  no path for file {n:#x}", file=sys.stderr)
            continue
        src = os.path.join(FILES, rel)
        if not os.path.exists(src):
            print(f"  missing {rel}", file=sys.stderr)
            continue
        stem = os.path.splitext(os.path.basename(rel))[0]
        d, warnings = pd_fpgun.export_model(src, texdir)
        ci = chrinfo(src)
        if ci is not None:
            d["chrinfo"] = ci
        total += write_pdm(d, os.path.join(OUT, stem + ".pdm"))
        index[str(n)] = stem
        for w in warnings:
            if "undecodable" in w:
                print(f"  {stem}: {w}", file=sys.stderr)
    with open(os.path.join(OUT, "index.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(index, fh, indent=0, sort_keys=True)

    table, _ = pd_fpgun.load_anim_table()
    ids = [c.eval("ANIM_01FC"), c.eval("ANIM_040D")]
    meta = pd_fpgun.export_anims(table, ids, os.path.join(OUT, "anims"))
    with open(os.path.join(OUT, "anims", "anims.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(meta, fh, indent=1)

    ntex = len(os.listdir(texdir))
    print(f"pd_menu_models: {len(index)} models ({total / 1e6:.1f} MB), {ntex} textures, anims {ids} -> {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
