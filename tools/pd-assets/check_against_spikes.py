#!/usr/bin/env python3
"""Regression check: the new `assets/` against the spikes' per-feature exports.

    python tools/pd-assets/check_against_spikes.py [old_repo]

M1 moved the spikes' assets into one layout. Moving must not change a byte, so
every texture, animation, sound, font and model the spikes shipped is compared
with its new home (`PD_OLD_REPO` or the argument, default: the old repo path in
CLAUDE.md). Known, intended differences are listed in KNOWN and reported, not
failed. Exit status 1 on any other difference.
"""

from __future__ import annotations

import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from pd_paths import out  # noqa: E402

OLD_DEFAULT = r"D:\Claude Code Projects\Hide and Seek Level Builder"

#: Intended differences, each with its reason.
KNOWN = {
    "unarmed": "invitem_unarmed joins the export for the fists (M4); g_MpWeapons does not list it",
    "sfx envelope": "the spikes exported some sounds raw (the menu set, grunts, footstep impacts) and "
                    "the gun set with the envelope; the pool bakes PD's playback envelope into every "
                    "non-looping sound (n_sndplayer.c applies it to all voices)",
    "bbox rodata": "the gun spike's chrcloaker/chrspeedpill/xrayspecs were exported before export_model "
                   "read BBOX rodata; their BBOX nodes now carry hitpart + box",
}

fails: list[str] = []
notes: list[str] = []


def same_bytes(a: str, b: str) -> bool:
    with open(a, "rb") as fa, open(b, "rb") as fb:
        return fa.read() == fb.read()


def check_textures(old: str) -> None:
    pool_dirs = ["pd_menu/textures", "pd_menu/models/tex", "weapons/pd_fp/textures", "weapons/pd_fp/fx",
                 "levels/pd_bg/ref/textures"]
    n = moved = 0
    for d in pool_dirs:
        folder = os.path.join(old, d)
        for f in sorted(os.listdir(folder)):
            if not f.endswith(".png"):
                continue
            n += 1
            stem = f[len("tex_"):-len(".png")]
            if len(stem) == 4 and all(c in "0123456789abcdef" for c in stem):
                new = out("textures", stem + ".png")
            else:  # tex_<model>_<index>.png: stored in the model file
                model, idx = stem.rsplit("_", 1)
                new = out("models", "tex", f"{model}_{idx}.png")
            if not os.path.exists(new):
                fails.append(f"texture {d}/{f}: missing at {os.path.relpath(new, out())}")
            elif not same_bytes(os.path.join(folder, f), new):
                fails.append(f"texture {d}/{f}: bytes differ from {os.path.relpath(new, out())}")
            else:
                moved += 1
    notes.append(f"textures: {moved}/{n} old PNGs byte-identical in the pool")


def check_anims(old: str) -> None:
    n = moved = 0
    index = json.load(open(out("anims", "index.json"), encoding="utf-8"))
    for d, meta_name in (("weapons/pd_fp/anims", None), ("pd_menu/models/anims", "anims.json")):
        folder = os.path.join(old, d)
        for f in sorted(os.listdir(folder)):
            if not f.endswith(".bin"):
                continue
            n += 1
            if same_bytes(os.path.join(folder, f), out("anims", f)):
                moved += 1
            else:
                fails.append(f"anim {d}/{f}: bytes differ")
        if meta_name:
            for k, m in json.load(open(os.path.join(folder, meta_name), encoding="utf-8")).items():
                e = index[f"{int(k):04x}"]
                for field in ("numframes", "bytesperframe", "headerlen", "framelen", "flags", "id"):
                    if e[field] != m[field]:
                        fails.append(f"anim {k} {field}: {e[field]} != old {m[field]}")
    weapons = json.load(open(os.path.join(old, "weapons/pd_fp/weapons.json"), encoding="utf-8"))
    for k, m in weapons["anims"].items():
        e = index[f"{int(k):04x}"]
        for field in ("numframes", "bytesperframe", "headerlen", "framelen", "flags", "id"):
            if e[field] != m[field]:
                fails.append(f"anim {k} {field}: {e[field]} != old {m[field]}")
    notes.append(f"anims: {moved}/{n} old bins byte-identical in the bank ({len(index)} in the bank)")


def check_sfx(old: str) -> None:
    new_m = json.load(open(out("sfx", "manifest.json"), encoding="utf-8"))
    game = os.path.join(old, "audio/pd/sfx")
    menu = os.path.join(old, "audio/pd/menu_sfx")
    n = moved = envelope = 0
    for folder in (game, menu):
        old_m = json.load(open(os.path.join(folder, "sfx_manifest.json"), encoding="utf-8"))
        for f in sorted(os.listdir(folder)):
            if not f.endswith(".wav"):
                continue
            n += 1
            key = f[:-4]
            if same_bytes(os.path.join(folder, f), out("sfx", f)):
                moved += 1
            elif not old_m[key]["envelope_baked"] and new_m[key]["envelope_baked"]:
                envelope += 1
            else:
                fails.append(f"sfx {os.path.basename(folder)}/{f}: bytes differ")
        for k, e in old_m.items():
            if k not in new_m:
                fails.append(f"sfx manifest key {k} missing")
                continue
            ne = dict(new_m[k])
            oe = dict(e)
            for drop in ("used_by", "envelope_baked", "source"):
                ne.pop(drop, None)
                oe.pop(drop, None)
            if ne != oe:
                fails.append(f"sfx manifest {k}: {oe} != {ne}")
    notes.append(f"sfx: {moved}/{n} old WAVs byte-identical, {envelope} differ by the baked envelope "
                 f"(known: {KNOWN['sfx envelope']})")


def check_weapons(old: str) -> None:
    a = json.load(open(os.path.join(old, "weapons/pd_fp/weapons.json"), encoding="utf-8"))
    b = json.load(open(out("data", "weapons.json"), encoding="utf-8"))
    # Intended: the export now has `invitem_unarmed` (the fists every player holds,
    # which g_MpWeapons does not list), and so its scripts and animations too.
    added = [w for w in b["weapons"] if w["weapon"] == "WEAPON_UNARMED"]
    b_weapons = [w for w in b["weapons"] if w["weapon"] != "WEAPON_UNARMED"]
    for k in sorted(set(a) - {"anims", "models"}):  # now the anim bank and models/index.json
        if k == "weapons":
            for x, y in zip(a[k], b_weapons):
                x, y = dict(x), dict(y)
                # `editor` pointed at the old repo's Perfect Gold dump, which this repo
                # does not have and nothing reads at run time.
                x.pop("editor", None)
                y.pop("editor", None)
                # Intended (M5): a projectile function names its model file and
                # the model's scale (`projectilemodelnum`, `g_ModelStates`).
                y["functions"] = [
                    {k: v for k, v in f.items() if k not in ("projectile_model", "projectile_model_scale")} if isinstance(f, dict) else f
                    for f in y.get("functions", [])
                ]
                if x != y:
                    fails.append(f"weapons.json {x['symbol']} differs")
            if len(a[k]) != len(b_weapons):
                fails.append("weapons.json weapon count differs")
        elif k == "scripts":
            if any(b[k].get(n) != v for n, v in a[k].items()):
                fails.append("weapons.json scripts differ")
            extra = sorted(set(b[k]) - set(a[k]))
            if extra and not all(n.startswith("invanim_punch") for n in extra):
                fails.append(f"weapons.json has unexpected new scripts {extra}")
        elif k == "anim_ids":
            if not set(a[k]) <= set(b[k]):
                fails.append("weapons.json lost anim ids")
        elif a[k] != b.get(k):
            fails.append(f"weapons.json {k} differs")
    notes.append(
        f"weapons.json: {len(b_weapons)} weapons and {len(a['scripts'])} gun scripts identical; "
        f"added {len(added)} (unarmed) and {len(b['scripts']) - len(a['scripts'])} punch scripts "
        f"({KNOWN['unarmed']})"
    )


def check_fonts_lang(old: str) -> None:
    for d in ("pd_menu/fonts", "weapons/pd_fp/fonts"):
        for f in os.listdir(os.path.join(old, d)):
            if not same_bytes(os.path.join(old, d, f), out("fonts", f)):
                fails.append(f"font {d}/{f} differs")
    for f, new in (("pd_menu/mpconfigs.bin", out("data", "mpconfigs.bin")),
                   ("pd_menu/mpstringsE.bin", out("lang", "mpstringsE.bin"))):
        if not same_bytes(os.path.join(old, f), new):
            fails.append(f"{f} differs")
    old_lang = json.load(open(os.path.join(old, "pd_menu/lang_en.json"), encoding="utf-8"))
    new_lang = json.load(open(out("lang", "en.json"), encoding="utf-8"))["banks"]
    for bank, strings in old_lang.items():
        if new_lang[bank]["strings"] != strings:
            fails.append(f"lang bank {bank} differs")
    notes.append(f"fonts, mpconfigs, mpstringsE identical; {len(old_lang)} old lang banks identical "
                 f"({len(new_lang)} banks now)")


# ---------------------------------------------------------------------------
# Models
# ---------------------------------------------------------------------------

VERT = struct.Struct("<fffHffBBBBBx")


def load_new(stem: str, dirpath: str | None = None) -> dict:
    dirpath = dirpath or out("models")
    head = json.load(open(os.path.join(dirpath, stem + ".json"), encoding="utf-8"))
    data = open(os.path.join(dirpath, stem + ".bin"), "rb").read()
    off = 0
    batches = []
    for b in head["batches"]:
        verts = []
        for _ in range(b["nverts"]):
            verts.append(list(VERT.unpack_from(data, off)))
            off += VERT.size
        idx = list(struct.unpack_from(f"<{b['nidx']}H", data, off))
        off += 2 * b["nidx"]
        batches.append({"node": b["node"], "material": b["material"], "verts": verts, "indices": idx})
    if off != len(data):
        fails.append(f"model {stem}: {len(data) - off} trailing bytes in .bin")
    head["batches"] = batches
    return head


def load_pdm(path: str) -> dict:
    data = open(path, "rb").read()
    hl = struct.unpack_from("<I", data, 4)[0]
    head = json.loads(data[8:8 + hl])
    off = 8 + hl
    batches = []
    for b in head["batches"]:
        verts = []
        for _ in range(b["nverts"]):
            verts.append(list(VERT.unpack_from(data, off)))
            off += VERT.size
        idx = list(struct.unpack_from(f"<{b['nidx']}H", data, off))
        off += 2 * b["nidx"]
        batches.append({"node": b["node"], "material": b["material"], "verts": verts, "indices": idx})
    head["batches"] = batches
    return head


def f32(x: float) -> float:
    return struct.unpack("<f", struct.pack("<f", x))[0]


def load_old_json(path: str) -> dict:
    d = json.load(open(path, encoding="utf-8"))
    for b in d["batches"]:
        b["verts"] = [[f32(v[0]), f32(v[1]), f32(v[2]), int(v[3]), f32(v[4]), f32(v[5]),
                       int(v[6]), int(v[7]), int(v[8]), int(v[9]), int(v[10])] for v in b["verts"]]
    return d


def node_equal(old: dict, new: dict, chrinfo: dict | None) -> bool:
    n = dict(new)
    if n.get("type") == "bbox" and "hitpart" not in old:  # KNOWN["bbox rodata"]
        n.pop("hitpart", None)
        n.pop("bbox", None)
    if n.get("type") == "chrinfo" and chrinfo is not None:
        # The chrinfo rodata moved from the model header onto its node.
        if n.pop("animpart", None) != chrinfo["animpart"] or n.pop("mtx", [None])[0] != chrinfo["mtx"]:
            return False
    return n == old


def compare_model(stem: str, old: dict, where: str, dirpath: str | None = None) -> bool:
    new = load_new(stem, dirpath)
    ok = True

    def bad(msg: str) -> None:
        nonlocal ok
        ok = False
        fails.append(f"model {where}/{stem}: {msg}")

    for k in ("name", "nummatrices", "parts", "materials"):
        if old.get(k) != new.get(k):
            bad(f"{k} differs")
    if "skel" in old and old["skel"] != new["skel"]:
        bad("skel differs")
    if len(old["nodes"]) != len(new["nodes"]):
        bad("node count differs")
    else:
        for i, (a, b) in enumerate(zip(old["nodes"], new["nodes"])):
            if not node_equal(a, b, old.get("chrinfo")):
                bad(f"node {i} differs: {a} vs {b}")
                break
    if len(old["batches"]) != len(new["batches"]):
        bad("batch count differs")
    else:
        for i, (a, b) in enumerate(zip(old["batches"], new["batches"])):
            if a["node"] != b["node"] or a["material"] != b["material"]:
                bad(f"batch {i} node/material differs")
            elif a["indices"] != b["indices"]:
                bad(f"batch {i} indices differ")
            elif a["verts"] != b["verts"]:
                bad(f"batch {i} vertices differ")
            if not ok:
                break
    if set(old["textures"]) != set(new["textures"]):
        bad(f"texture ids differ: {sorted(old['textures'])} vs {sorted(new['textures'])}")
    return ok


def check_models(old: str) -> None:
    n = same = 0
    folder = os.path.join(old, "pd_menu/models")
    for f in sorted(os.listdir(folder)):
        if f.endswith(".pdm"):
            n += 1
            same += compare_model(f[:-4], load_pdm(os.path.join(folder, f)), "pd_menu")
    folder = os.path.join(old, "weapons/pd_fp/models")
    for f in sorted(os.listdir(folder)):
        if f.endswith(".json"):
            n += 1
            same += compare_model(f[:-5], load_old_json(os.path.join(folder, f)), "pd_fp")
    new = json.load(open(out("models", "index.json"), encoding="utf-8"))
    notes.append(f"models: {same}/{n} old models identical in nodes, parts, materials and every vertex "
                 f"and index ({len(new)} models now)")


def check_stages(old: str) -> None:
    """The stage BGs against the Complex spike's `levels/pd_bg/<code>/bg.json`:
    the same nodes (rooms, layers, BSP trees), materials, and every vertex and
    index; textures now come from the pool (checked by `check_textures`)."""
    n = same = 0
    folder = os.path.join(old, "levels", "pd_bg")
    for code in sorted(os.listdir(folder)):
        path = os.path.join(folder, code, "bg.json")
        if os.path.exists(path):
            n += 1
            same += compare_model("bg", load_old_json(path), f"stages/{code}", out("stages", code))
    notes.append(f"stages: {same}/{n} old BG exports identical in nodes, materials and every vertex and index")


def main() -> int:
    repo = os.environ.get("PD_OLD_REPO") or (sys.argv[1] if len(sys.argv) > 1 else OLD_DEFAULT)
    old = os.path.join(repo, "native", "assets")
    if not os.path.isdir(old):
        raise SystemExit(f"no old assets at {old}")
    check_textures(old)
    check_anims(old)
    check_sfx(old)
    check_weapons(old)
    check_fonts_lang(old)
    check_models(old)
    check_stages(old)
    for n in notes:
        print(n)
    for f in fails[:60]:
        print("FAIL", f)
    if len(fails) > 60:
        print(f"... and {len(fails) - 60} more")
    print("OK" if not fails else f"{len(fails)} differences")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
