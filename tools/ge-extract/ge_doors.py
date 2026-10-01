"""Every door GoldenEye's setups place, as a catalogue for the level editor
(`pd_edit`): each door model into `<custom>/models/` (`ge_model`) and
`<custom>/ge/doors.json`, in the format of PD's `assets/data/doors.json`
(`tools/pd-assets/pd_doors.py`: a model, its templates (the distinct door rows
it is placed with, as PD rows, `ge_setup.door_row`) with the median size of
their boxes, and the levels it is seen on).

The setups are the decomp's (`assets/obseg/setup/*.c` and `u/*.c`), each
level's pads in its BG units (`/ levelscale` is centimetres, `bg.c:184`). Door
types PD doesn't run are left out as PD's catalogue leaves them
(`pd_doors.UNPORTED_TYPES`: the fall-away hatches), and so is a model placed
only as those.
"""

from __future__ import annotations

import glob
import json
import os
import statistics
import sys

from ge_model import CUSTOM_MODELNUM_BASE
from ge_rom import PD_ASSETS_TOOLS, Ge, GeError

sys.path.insert(0, PD_ASSETS_TOOLS)
import pd_doors  # noqa: E402

import ge_model  # noqa: E402
import ge_setup  # noqa: E402

EXPORTER = "tools/ge-extract/ge_doors.py"

#: A setup's code (`Usetup<code>Z`, `Ump_setup<code>Z`) where it isn't its
#: level's BG code (`bg.c:184`).
SETUP_BG = {"statue": "stat", "control": "arec", "depot": "depo", "sevb": "sev", "sevbunker": "sev", "sevxb": "sevx"}


def setups(ge: Ge) -> list[str]:
    """Every setup's name, the root's before `u/`'s of the same name."""
    root = os.path.join(ge.decomp, "assets", "obseg", "setup")
    names: list[str] = []
    for path in sorted(glob.glob(os.path.join(root, "*.c"))) + sorted(glob.glob(os.path.join(root, "u", "*.c"))):
        name = os.path.basename(path)[:-2]
        if name not in names:
            names.append(name)
    return names


def level_of(ge: Ge, setup: str) -> tuple[str, float] | None:
    """The `LEVELID_*` name (without `LEVELID_`) and level scale of `setup`'s level."""
    code = setup.removeprefix("Ump_setup").removeprefix("Usetup").removesuffix("Z")
    bg = SETUP_BG.get(code, code)
    for levelid, row in ge.levels().items():
        if row["code"] == bg:
            return levelid.removeprefix("LEVELID_"), row["levelscale"]
    return None


def build(ge: Ge, images, custom: str, scale: float = 1.0) -> list[str]:
    """Export every GE door model and write `<custom>/ge/doors.json`. `scale`:
    GE world units to PD centimetres. Returns the report."""
    names = ge.props()
    found: dict[int, dict] = {}
    skipped: dict[int, set[str]] = {}
    report = []
    for name in setups(ge):
        lv = level_of(ge, name)
        if lv is None:
            report.append(f"  {name}: no level (its doors' sizes are not known), skipped")
            continue
        levelname, levelscale = lv
        setup = ge_setup.Setup(ge, name)
        k = scale / levelscale
        for ty, w in setup.props:
            if ty != "Door":
                continue
            obj, pad = w[1] >> 16, w[1] & 0xFFFF
            row = ge_setup.door_row(w, scale)
            doortype = row["doortype"]
            if doortype in pd_doors.UNPORTED_TYPES:
                skipped.setdefault(obj, set()).add(pd_doors.UNPORTED_TYPES[doortype])
                continue
            t = {
                "doortype": doortype, "doorflags": row["doorflags"] & ~pd_doors.DOORFLAG_ROTATEDPAD,
                "flags": row["flags"] & pd_doors.KEEP_FLAGS, "flags2": row["flags2"] & pd_doors.KEEP_FLAGS2,
                "maxfrac": row["maxfrac"], "perimfrac": row["perimfrac"], "accel": row["accel"], "decel": row["decel"],
                "maxspeed": row["maxspeed"], "autoclosetime": row["autoclosetime"], "unk88": row["unk88"],
                "soundtype": (row["unkc4"] >> 8) & 0xFF,
            }
            key = tuple(t[x] for x in pd_doors.ROW_KEYS)
            e = found.setdefault(obj, {"templates": {}, "seen": set()})
            e["seen"].add(levelname)
            sizes = e["templates"].setdefault(key, [])
            if pad < len(setup.bpads):
                b = setup.bpads[pad]["bbox"]
                sizes.append(((b[3] - b[2]) * k, (b[5] - b[4]) * k, (b[1] - b[0]) * k))
    doors = []
    for obj in sorted(found):
        stem, model, entry, warnings = ge_model.export(ge, images, obj)
        ge_model.write(custom, stem, model, entry)
        report.append(f"  {stem}: MODEL {entry['modelnum']:#x}, {entry['tris']} triangles" + "".join(f"; WARN {x}" for x in warnings))
        templates = []
        for key, sizes in found[obj]["templates"].items():
            size = [round(statistics.median(s[i] for s in sizes)) for i in range(3)] if sizes else [100, 200, 10]
            templates.append({"count": max(len(sizes), 1), "size": size, "row": dict(zip(pd_doors.ROW_KEYS, key))})
        templates.sort(key=lambda t: (-t["count"], [t["row"][x] for x in pd_doors.ROW_KEYS]))
        doors.append({"stem": stem, "model": CUSTOM_MODELNUM_BASE + obj, "name": names[obj],
                      "skel": model["skel"], "seen": sorted(found[obj]["seen"]), "templates": templates})
    for obj, kinds in sorted(skipped.items()):
        if obj not in found:
            report.append(f"  prop {obj} ({names[obj]}) left out: placed only as a {'/'.join(sorted(kinds))} door (not ported)")
    path = os.path.join(custom, "ge", "doors.json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump({"format": "pd-doors/1", "source": ge.rel(os.path.join(ge.decomp, "assets", "obseg", "setup")), "exporter": EXPORTER,
                   "doors": doors}, fh, indent=1)
        fh.write("\n")
    report.insert(0, f"GoldenEye doors: {len(doors)} models, {sum(len(d['templates']) for d in doors)} templates -> {path}")
    return report


if __name__ == "__main__":
    raise SystemExit("run through ge_extract.py --doors")
