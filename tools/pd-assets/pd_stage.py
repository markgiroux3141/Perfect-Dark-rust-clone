#!/usr/bin/env python3
"""Export a Combat Simulator stage into `assets/stages/<code>/`: its textured BG,
its collision tiles, its pads and waypoint graph, and its MP setup.

| File | From | Read by |
|------|------|---------|
| `bg.json` + `bg.bin` | `files/bgdata/bg_<code>.seg`, interpreted by `pd_bg.build` | `pd_core::model::ModelDef::load_file` (the one model format, `pd_models.py`), drawn by `pd_render::bg` |
| `tiles.json` | `tiles/<code>.json` (the decomp's export of `bg_<code>_tilesZ`) | `pd_sim::stage::Stage::load` |
| `pads.json` | `pads/<code>.json` (`bg_<code>_padsZ`) | the same |
| `setup.json` | `src/setups/mp_setup<code>.c` (`Ump_setup<code>Z`) | the same |

**bg.json** is a `pd-model/1` header (see `pd_models.py`) with one POSITION root
and one DL node per room and layer (`room`, `layer` "opa"/"xlu", and `tree`, the
BSP of a layer with parent blocks, `pd_bg.bsp_tree`), vertices in world
centimetres, textures from the pool (`textures/<num>.png`). Extra header keys:
`stage`, `units`, `env` (fog/transparency, `env.c`), `rooms` (`g_BgRooms` pos,
brightness range, the room's opa/xlu node, bounding box), `section2_textures`.
Not exported yet (M9, room lighting and animated textures): the per-vertex colour
index `room_highlight` rescales, and the `dyntex` s/t.

**tiles.json** (`pd-tiles/1`): `rooms` (every room number the tiles file names,
ascending) and `tiles[]`, each `{room, flags, floortype, floorcol, verts}`:
`flags` are PD's `GEOFLAG_*` bits (`constants.h:1189`) rebuilt from the JSON's
booleans, `floortype` is `FLOORTYPE_*` (`constants.h:961`), `verts` the outline
in cm. The tiles keep the decomp file's order within a room, rooms ascending.

**pads.json** (`pd-pads/1`): `pads[]` `{pos, look, up, flags, bbox, liftnum}`
with `flags` PD's `PADFLAG_*` bits (`constants.h:3321`); `waypoints[]` `{pad,
group, neighbours}` and `waygroups[]` `{neighbours}`, neighbours as PD encodes
them (`id | WPSEGFLAG_OUTWARDSONLY 0x4000 | WPSEGFLAG_INWARDSONLY 0x8000`,
`padhalllv.c:44`); `cover[]` `{pos, look, special}`.

**setup.json** (`pd-setup/1`): `stage` (`STAGE_*` name and number), `intro[]`
and `props[]`, one object per macro in the setup file, `{type, <param>: value}`
with the parameter names of the macro's `#define` (`include/intro.h`,
`include/props.h`). Pads are pad numbers; other arguments are evaluated where
the headers define them, else kept as their source text.

Usage:
    python tools/pd-assets/pd_stage.py              # every stage in pd_bg.STAGES
"""

from __future__ import annotations

import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pd_bg  # noqa: E402
import pd_menu_gen as gen  # noqa: E402
import pd_models  # noqa: E402
from pd_paths import asset, decomp_rel, out, src  # noqa: E402

EXPORTER = "tools/pd-assets/pd_stage.py"

#: The tiles JSON's booleans -> `GEOFLAG_*` (constants.h:1189-1204).
GEOFLAGS = {
    "flag0001": 0x0001,  # GEOFLAG_FLOOR1
    "flag0002": 0x0002,  # GEOFLAG_FLOOR2
    "flag0004": 0x0004,  # GEOFLAG_WALL
    "flag0008": 0x0008,  # GEOFLAG_BLOCK_SIGHT
    "flag0010": 0x0010,  # GEOFLAG_BLOCK_SHOOT
    "flag0020": 0x0020,  # GEOFLAG_LIFTFLOOR
    "ladder": 0x0040,  # GEOFLAG_LADDER
    "flag0080": 0x0080,  # GEOFLAG_RAMPWALL
    "flag0100": 0x0100,  # GEOFLAG_SLOPE
    "underwater": 0x0200,  # GEOFLAG_UNDERWATER
    "flag0400": 0x0400,  # GEOFLAG_0400
    "aibotcrouch": 0x0800,  # GEOFLAG_AIBOTCROUCH
    "aibotduck": 0x1000,  # GEOFLAG_AIBOTDUCK
    "flag2000": 0x2000,  # GEOFLAG_STEP
    "die": 0x4000,  # GEOFLAG_DIE
    "climbableledge": 0x8000,  # GEOFLAG_LADDER_PLAYERONLY
}

#: `FLOORTYPE_*` (constants.h:961-969) by the tiles JSON's name.
FLOORTYPES = {"default": 0, "wood": 1, "stone": 2, "carpet": 3, "metal": 4, "mud": 5, "water": 6, "dirt": 7, "snow": 8}

#: The pads JSON's booleans -> `PADFLAG_*` (constants.h:3321-3338).
PADFLAGS = {
    "aiwaitlift": 0x0400,
    "aionlift": 0x0800,
    "aiwalkdirect": 0x1000,
    "aidrop": 0x2000,
    "aicrouch": 0x4000,
    "aiignorey": 0x8000,
    "aiduck": 0x10000,
}

WPSEGFLAG_OUTWARDSONLY = 0x4000  # padhalllv.c:44
WPSEGFLAG_INWARDSONLY = 0x8000  # padhalllv.c:45


def id_suffix(name: str) -> int:
    """The number at the end of a generated name: `PAD_REF_001C` -> 0x1c. The
    decomp's asset tool numbers pads, waypoints, waygroups and rooms in order."""
    return int(name.rsplit("_", 1)[1], 16)


def write_json(path: str, obj: dict) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(obj, fh, separators=(",", ":"))
        fh.write("\n")


# ---------------------------------------------------------------------------
# BG
# ---------------------------------------------------------------------------


def bg_file(stem: str) -> tuple[int, str]:
    """(FILE_* number, name) of `bgdata/bg_<stem>.seg` (files/list.c, files.h)."""
    text = gen.read(asset("files", "list.c"))
    m = re.search(rf"/\*0x([0-9a-f]+)\*/\s*\"bgdata/bg_{stem}\.seg\"", text)
    if not m:
        raise SystemExit(f"bg_{stem}.seg is not in files/list.c")
    num = int(m.group(1), 16)
    return num, "FILE_" + gen.file_names().get(num, f"{num:04X}")


def export_bg(stem: str, pool: pd_models.TexturePool) -> dict:
    d = pd_bg.build(stem)
    for line in d["summary"]:
        print(f"  {line}")
    for w in d["warnings"]:
        print(f"  WARN {w}", file=sys.stderr)
    textures = {}
    for texnum in d["used"]:
        e = pool.pool(texnum)
        # The BG's texconfig is the texture itself (pd_bg.texture_configs).
        textures[str(texnum)] = {"file": f"textures/{texnum:04x}.png", "w": e["w"], "h": e["h"],
                                 "cfg_w": e["w"], "cfg_h": e["h"], "levels": e["numlods"], "source": "pd"}
    filenum, filename = bg_file(stem)
    model = {k: d[k] for k in ("name", "source", "nummatrices", "nodes", "parts", "materials", "batches")}
    model.update(skel=None, textures=textures)
    extra = {k: d[k] for k in ("stage", "units", "env", "rooms", "section2_textures")}
    nbytes, tris = pd_models.write_model(model, "bg", filenum, filename, out("stages", stem), EXPORTER, extra)
    return {"bg_tris": tris, "bg_bytes": nbytes, "bg_textures": len(textures)}


# ---------------------------------------------------------------------------
# Tiles, pads
# ---------------------------------------------------------------------------


def export_tiles(stem: str) -> dict:
    path = asset("tiles", f"{stem}.json")
    with open(path, encoding="utf-8") as fh:
        rooms = json.load(fh)["rooms"]
    out_tiles = []
    for name in sorted(rooms, key=id_suffix):
        for t in rooms[name]:
            flags = 0
            for key, bit in GEOFLAGS.items():
                if t.get(key):
                    flags |= bit
            verts = [[v["x"], v["y"], v["z"]] for v in t["vertices"]]
            if len(verts) < 3:
                raise SystemExit(f"{name}: a tile with {len(verts)} vertices")
            out_tiles.append({"room": id_suffix(name), "flags": flags,
                              "floortype": FLOORTYPES[t.get("floortype", "default")],
                              "floorcol": t.get("floorcolour", 0), "verts": verts})
    write_json(out("stages", stem, "tiles.json"), {
        "format": "pd-tiles/1", "source": decomp_rel(path), "exporter": EXPORTER,
        "rooms": sorted(id_suffix(n) for n in rooms), "tiles": out_tiles,
    })
    return {"tiles": len(out_tiles), "tile_rooms": len(rooms)}


def segments(entries: list[dict], key: str) -> list[int]:
    return [id_suffix(n[key]) | (WPSEGFLAG_OUTWARDSONLY if n.get("flag4000") else 0)
            | (WPSEGFLAG_INWARDSONLY if n.get("flag8000") else 0) for n in entries]


def export_pads(stem: str) -> dict:
    path = asset("pads", f"{stem}.json")
    with open(path, encoding="utf-8") as fh:
        j = json.load(fh)
    for what in ("pads", "waypoints", "waygroups", "cover"):
        for i, e in enumerate(j.get(what, [])):
            if id_suffix(e["id"]) != i:
                raise SystemExit(f"{stem} {what}: {e['id']} is at index {i}")
    pads = []
    for p in j["pads"]:
        flags = 0
        for key, bit in PADFLAGS.items():
            if p.get(key):
                flags |= bit
        pads.append({"pos": p["pos"], "look": p["dir"], "up": p["up"], "flags": flags,
                     "bbox": [p["xmin"], p["xmax"], p["ymin"], p["ymax"], p["zmin"], p["zmax"]],
                     "liftnum": p.get("liftnum", 0)})
    waypoints = [{"pad": id_suffix(w["pad"]), "group": id_suffix(w["waygroup"]),
                  "neighbours": segments(w.get("neighbours", []), "waypoint")} for w in j["waypoints"]]
    waygroups = [{"neighbours": segments(g.get("neighbours", []), "waygroup")} for g in j["waygroups"]]
    cover = [{"pos": c["pos"], "look": c["dir"], "special": c.get("special", 0)} for c in j.get("cover", [])]
    for i, w in enumerate(waypoints):
        if w["pad"] >= len(pads) or w["group"] >= len(waygroups):
            raise SystemExit(f"{stem} waypoint {i:#x}: pad or group out of range")
        if any((s & 0x3FFF) >= len(waypoints) for s in w["neighbours"]):
            raise SystemExit(f"{stem} waypoint {i:#x}: a neighbour out of range")
    write_json(out("stages", stem, "pads.json"), {
        "format": "pd-pads/1", "source": decomp_rel(path), "exporter": EXPORTER,
        "pads": pads, "waypoints": waypoints, "waygroups": waygroups, "cover": cover,
    })
    return {"pads": len(pads), "waypoints": len(waypoints), "waygroups": len(waygroups), "cover": len(cover)}


# ---------------------------------------------------------------------------
# The MP setup
# ---------------------------------------------------------------------------


def macro_params(*headers: str) -> dict[str, list[str]]:
    """`#define name(a, b, ...)` -> [a, b, ...] from the setup headers."""
    params = {}
    for h in headers:
        for m in re.finditer(r"^#define\s+(\w+)\(([^)]*)\)", gen.read(src("include", h)), re.M):
            params[m.group(1)] = [p.strip() for p in m.group(2).split(",") if p.strip()]
    return params


def calls(body: str) -> list[tuple[str, list[str]]]:
    """The macro calls in an initialiser body, in order: `name(args)` or a bare `name`."""
    body = gen.strip_comments(body)
    outl = []
    for m in re.finditer(r"\b([a-z_][a-z0-9_]*)\b\s*(\(([^()]*)\))?", body):
        name, args = m.group(1), m.group(3)
        outl.append((name, [a.strip() for a in args.split(",")] if args is not None else []))
    return outl


def export_setup(stem: str, stage_name: str) -> dict:
    path = src("setups", f"mp_setup{stem}.c")
    text = gen.read(path)
    c = gen.Consts()
    for h in ("constants.h", "files.h"):
        c.load_header(src("include", h))
    params = macro_params("intro.h", "props.h")
    padnums = {}
    with open(asset("pads", f"{stem}.json"), encoding="utf-8") as fh:
        for i, p in enumerate(json.load(fh)["pads"]):
            padnums[p["id"]] = i

    def value(arg: str):
        if arg in padnums:
            return padnums[arg]
        try:
            return c.eval(arg)
        except Exception:  # noqa: BLE001 (a name the headers do not define)
            return arg

    def section(name: str, end: str) -> list[dict]:
        m = re.search(rf"\b{name}\[\]\s*=\s*\{{(.*?)\n\}};", text, re.S)
        if not m:
            raise SystemExit(f"{decomp_rel(path)}: no {name}[]")
        entries = []
        for macro, args in calls(m.group(1)):
            if macro == end:
                break
            names = params.get(macro)
            if names is None or len(names) != len(args):
                raise SystemExit(f"{decomp_rel(path)}: {macro}({', '.join(args)}) is not a known macro")
            entries.append({"type": macro, **{k: value(a) for k, a in zip(names, args)}})
        return entries

    intro = section("intro", "endintro")
    props = section("props", "endprops")
    write_json(out("stages", stem, "setup.json"), {
        "format": "pd-setup/1", "source": decomp_rel(path), "exporter": EXPORTER,
        "stage": {"name": stage_name, "num": c.eval(stage_name), "code": stem},
        "intro": intro, "props": props,
    })
    return {"spawns": sum(1 for e in intro if e["type"] == "spawn"), "props": len(props)}


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------


def export_all(pool: pd_models.TexturePool) -> dict:
    """Every stage in `pd_bg.STAGES`. Returns counts for MANIFEST.json."""
    counts: dict[str, dict] = {}
    for stem, (stage_name, _) in sorted(pd_bg.STAGES.items()):
        c = {}
        c.update(export_bg(stem, pool))
        c.update(export_tiles(stem))
        c.update(export_pads(stem))
        c.update(export_setup(stem, stage_name))
        counts[stem] = c
        print(f"pd_stage {stem}: {json.dumps(c)}")
    return {"stages": counts}


def main() -> int:
    pool = pd_models.TexturePool()
    # The pool's index is rebuilt by build_assets.py; alone, this run only
    # (re)writes the stages and the textures they use.
    export_all(pool)
    return 0


if __name__ == "__main__":
    sys.exit(main())
