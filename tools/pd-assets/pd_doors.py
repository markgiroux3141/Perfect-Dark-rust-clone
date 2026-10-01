#!/usr/bin/env python3
"""Every door PD's setups place, as a catalogue for the level editor
(`pd_edit`): `data/doors.json`, and the list of door models `pd_models.py`
exports (with their display lists' vertex tables, for `door_calc_texturemap`).

A door in a setup is a `door()` row (`include/props.h`, made by
`setup_create_door`, `setup.c:944`) on a pad with a box: the model is
stretched to the box (its x along the pad's up, across the doorway; y along
its look, up the doorway; z along its normal, the door's thickness). So a door
is a model, a box and the row's numbers. Each model's catalogue entry keeps
the distinct rows it is placed with ("templates", most used first), with the
median size of the boxes each is placed in, and the stages it is seen on.

Only what makes a door a door is kept of a row: its type, door flags, speeds,
sound, auto-close time and glass distances, and of the object flags those a
Combat Simulator door keeps (the portal, the swing, two-way, the colour and AI
sight bits; the locks, keys and mission states, starting open among them, are
dropped, as GoldenEye's own multiplayer setups drop them). Door types that aren't ported
(`DOORTYPE_FALLAWAY`, a hatch that drops as a projectile once open;
`DOORTYPE_LASER`) are left out, and so is a model placed only as those.

    doors.json (`pd-doors/1`): {doors: [{stem, model (MODEL_*), name, skel,
        seen: [STAGE_* without STAGE_], templates: [{count, size: [width,
        height, thickness] cm, row: {doortype, doorflags, flags, flags2,
        maxfrac, perimfrac, accel, decel, maxspeed, autoclosetime, unk88,
        soundtype}}]}]}

`row` holds the setup's own integers (16.16 fractions, accel/decel / 65536000,
`unk88` = xludist << 16 | opadist), so a door made from it is PD's to the bit.
"""

from __future__ import annotations

import glob
import json
import os
import re
import statistics
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pd_menu_gen as gen  # noqa: E402
from pd_paths import asset, decomp_rel, out, src  # noqa: E402

EXPORTER = "tools/pd-assets/pd_doors.py"

DOORTYPE_FALLAWAY, DOORTYPE_LASER = 8, 11  # constants.h:840, :843
DOORFLAG_ROTATEDPAD = 0x0040  # constants.h:815
UNPORTED_TYPES = {DOORTYPE_FALLAWAY: "fall-away", DOORTYPE_LASER: "laser"}

#: `obj->flags` bits a door template keeps (constants.h): 0x10 (a chr's door
#: sight test, chr.c:3019), ORTHOGONAL, IGNOREFLOORCOLOUR, IGNOREROOMCOLOUR,
#: AISEETHROUGH, DOOR_HASPORTAL, DOOR_OPENTOFRONT, DOOR_TWOWAY. Not
#: DOOR_KEEPOPEN: a door open at the start is a mission's state (Caverns'
#: eyelids and irises start open until its scripts shut them), so a door made
#: from the catalogue starts shut.
KEEP_FLAGS = 0x10 | 0x200 | 0x400 | 0x1000 | 0x4000000 | 0x10000000 | 0x20000000 | 0x80000000
#: `obj->flags2` bits kept: SHOOTTHROUGH, IMMUNETOGUNFIRE, BULLETPROOF,
#: IMMUNETOEXPLOSIONS, AICANNOTUSE, DOOR_ALTCOORDSYSTEM.
KEEP_FLAGS2 = 0x8000 | 0x4000 | 0x100000 | 0x200000 | 0x20000000 | 0x80000000

#: Door models left out, and why.
LEFT_OUT = {
    # CI's hidden door: both its textures run past the end of its file
    # (`pd_models.py` can't decode them), so it would draw untextured.
    "MODEL_SECRETINDOOR": "its textures run past the end of its file",
}

ROW_KEYS = ("doortype", "doorflags", "flags", "flags2", "maxfrac", "perimfrac", "accel", "decel", "maxspeed",
            "autoclosetime", "unk88", "soundtype")


def _consts() -> gen.Consts:
    c = gen.Consts()
    for h in ("constants.h", "files.h"):
        c.load_header(src("include", h))
    return c


def _params() -> dict[str, list[str]]:
    import pd_stage  # noqa: PLC0415 (pd_stage imports pd_models, which imports this)

    return pd_stage.macro_params("intro.h", "props.h")


def _stage_names(c: gen.Consts) -> dict[str, str]:
    """Stage code -> its `STAGE_*` name without `STAGE_` (`stagetable.c`)."""
    text = gen.read(src("game", "stagetable.c"))
    names = {}
    for m in re.finditer(r"/\*0x[0-9a-f]+\*/\s*STAGE_(\w+)\s*,[^,]*,[^,]*,[^,]*,[^,]*,[^,]*,\s*FILE_BG_(\w+?)_SEG", text):
        names.setdefault(m.group(2).lower(), m.group(1))
    return names


def _u32(v) -> int:
    return int(v) & 0xFFFFFFFF


def _s32(v) -> int:
    v = _u32(v)
    return v - (1 << 32) if v & 0x80000000 else v


def scan(c: gen.Consts | None = None) -> tuple[dict[int, dict], list[str]]:
    """Every setup's doors: MODEL_* -> {templates: {row tuple: [sizes]},
    seen: {stage code}}; and report lines."""
    import pd_stage  # noqa: PLC0415

    c = c or _consts()
    params = _params()["door"]
    found: dict[int, dict] = {}
    skipped: dict[int, set[str]] = {}
    files = sorted(glob.glob(src("setups", "setup*.c")) + glob.glob(src("setups", "mp_setup*.c")))
    for path in files:
        base = os.path.basename(path)[:-2]
        stem = base[len("mp_setup"):] if base.startswith("mp_setup") else base[len("setup"):]
        text = gen.preprocess(gen.read(path))
        m = re.search(r"\bprops\[\]\s*=\s*\{(.*?)\n\};", text, re.S)
        if not m:
            continue
        pads_path = asset("pads", f"{stem}.json")
        pads, padnums = [], {}
        if os.path.exists(pads_path):
            with open(pads_path, encoding="utf-8") as fh:
                pads = json.load(fh)["pads"]
            padnums = {p["id"]: i for i, p in enumerate(pads)}
        for macro, args in pd_stage.calls(m.group(1)):
            if macro != "door" or len(args) != len(params):
                continue
            a = dict(zip(params, args))
            if a["model"] in LEFT_OUT:
                skipped.setdefault(c.eval(a["model"]), set()).add(LEFT_OUT[a["model"]])
                continue
            ev = {k: c.eval(v) for k, v in a.items() if k not in ("pad",)}
            model = ev["model"]
            doortype = ev["doortype"] & 0xFFFF
            if doortype in UNPORTED_TYPES:
                skipped.setdefault(model, set()).add(UNPORTED_TYPES[doortype])
                continue
            unkc4 = _u32(ev["unkc4"])
            row = {
                "doortype": doortype, "doorflags": (ev["doorflags"] & 0xFFFF) & ~DOORFLAG_ROTATEDPAD,
                "flags": _u32(ev["flags"]) & KEEP_FLAGS, "flags2": _u32(ev["flags2"]) & KEEP_FLAGS2,
                "maxfrac": _s32(ev["maxfrac"]), "perimfrac": _s32(ev["perimfrac"]), "accel": _s32(ev["accel"]),
                "decel": _s32(ev["decel"]), "maxspeed": _s32(ev["maxspeed"]), "autoclosetime": _s32(ev["autoclosetime"]),
                "unk88": _u32(ev["unk88"]), "soundtype": (unkc4 >> 8) & 0xFF,
            }
            key = tuple(row[k] for k in ROW_KEYS)
            e = found.setdefault(model, {"templates": {}, "seen": set()})
            sizes = e["templates"].setdefault(key, [])
            e["seen"].add(stem)
            pad = pads[padnums[a["pad"]]] if a["pad"] in padnums else None
            if pad is not None:
                sizes.append((pad["ymax"] - pad["ymin"], pad["zmax"] - pad["zmin"], pad["xmax"] - pad["xmin"]))
    report = [f"pd_doors: {len(found)} door models in {len(files)} setups"]
    for model, kinds in sorted(skipped.items()):
        if model not in found:
            report.append(f"  MODEL {model:#x} left out: {'; '.join(sorted(k if ' ' in k else f'placed only as a {k} door (not ported)' for k in kinds))}")
    return found, report


def door_models(c: gen.Consts | None = None) -> list[int]:
    """Every MODEL_* the catalogue holds, sorted."""
    return sorted(scan(c)[0])


def export(c: gen.Consts | None = None) -> dict:
    """`data/doors.json`, after `pd_models.py` has written the models and
    their index (each door model's stem). Returns counts for MANIFEST.json."""
    c = c or _consts()
    with open(out("models", "index.json"), encoding="utf-8") as fh:
        index = json.load(fh)
    found, report = scan(c)
    stem_of = {row["modelnum"]: stem for stem, row in index.items() if "modelnum" in row}
    names: dict[int, str] = {}
    for k in c.defs:
        if k.startswith("MODEL_"):
            try:
                names.setdefault(c.eval(k), k)
            except Exception:  # noqa: BLE001 (not a number)
                pass
    stage_names = _stage_names(c)
    doors = []
    for model in sorted(found):
        stem = stem_of.get(model)
        if stem is None:
            raise SystemExit(f"door MODEL {model:#x} was not exported (pd_models.model_list)")
        with open(out("models", stem + ".json"), encoding="utf-8") as fh:
            skel = json.load(fh)["skel"]
        templates = []
        for key, sizes in found[model]["templates"].items():
            size = [round(statistics.median(s[k] for s in sizes)) for k in range(3)] if sizes else [100, 200, 10]
            templates.append({"count": max(len(sizes), 1), "size": size, "row": dict(zip(ROW_KEYS, key))})
        templates.sort(key=lambda t: (-t["count"], [t["row"][k] for k in ROW_KEYS]))
        seen = sorted({stage_names.get(s, s.upper()) for s in found[model]["seen"]})
        doors.append({"stem": stem, "model": model, "name": names.get(model, f"MODEL_{model:X}"), "skel": skel,
                      "seen": seen, "templates": templates})
    os.makedirs(out("data"), exist_ok=True)
    with open(out("data", "doors.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump({"format": "pd-doors/1", "source": decomp_rel(src("setups")), "exporter": EXPORTER, "doors": doors}, fh, indent=1)
        fh.write("\n")
    for line in report:
        print(line)
    return {"door_models": len(doors), "door_templates": sum(len(d["templates"]) for d in doors)}


if __name__ == "__main__":
    print(export())
