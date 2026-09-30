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
centimetres, textures from the pool (`textures/<num>.png`). Each batch's header
entry also carries `leaf` (its block within the layer), `cidx` (per vertex, the
index into the room's colour table that `room_highlight` rescales) and, for the
animated textures, `dyntex` (the kind), `st` (the raw s,t `dyntex_tick_room`
rewrites) and `stscale` (each vertex's `G_TEXTURE` s/t scale: U = s × scale
>> 16, in 1/32 texels). Extra header keys: `stage`, `units`, `env` (fog/transparency,
`env.c`), `rooms` (`g_BgRooms` pos and brightness range, section 3's bounding
box and light count, the room's opa/xlu node), `portals` (`g_BgPortals`: the
two rooms, flags, vertices), `bgcmds` (`g_BgCommands`), `lights` (the lights
file data, `struct light`), `section2_textures`.

**tiles.json** (`pd-tiles/1`): `rooms` (every room number the tiles file names,
ascending) and `tiles[]`, each `{room, flags, floortype, floorcol, verts}`:
`flags` are PD's `GEOFLAG_*` bits (`constants.h:1189`) rebuilt from the JSON's
booleans, `floortype` is `FLOORTYPE_*` (`constants.h:961`), `verts` the outline
in cm. The tiles keep the decomp file's order within a room, rooms ascending.

**pads.json** (`pd-pads/1`): `pads[]` `{pos, look, up, flags, bbox, liftnum}`
with `flags` PD's `PADFLAG_*` bits (`constants.h:3321`, `PADFLAG_HASBBOXDATA`
as `mkpads` sets it); `waypoints[]` `{pad,
group, neighbours}` and `waygroups[]` `{neighbours}`, neighbours as PD encodes
them (`id | WPSEGFLAG_OUTWARDSONLY 0x4000 | WPSEGFLAG_INWARDSONLY 0x8000`,
`padhalllv.c:44`); `cover[]` `{pos, look, special}`.

**setup.json** (`pd-setup/1`): `stage` (`STAGE_*` name and number, and `table`,
its `g_Stages` row by `struct stagetableentry`'s field names), `intro[]`
and `props[]`, one object per macro in the setup file, `{type, <param>: value}`
with the parameter names of the macro's `#define` (`include/intro.h`,
`include/props.h`). Pads are pad numbers; other arguments are evaluated where
the headers define them, else kept as their source text. `bgai[]`: the
background AI lists (`ailists[]` from id 0x1000), `{id, cmds[]}`, each command
`{type, <param>: value}` (only the few an MP setup uses: `activate_lift`,
`set_ailist`, `set_wind_speed` and the simulant set-up).

Usage:
    python tools/pd-assets/pd_stage.py              # every stage in pd_bg.STAGES
    PD_ASSETS_OUT=<dir> python tools/pd-assets/pd_stage.py --base <code> <STAGE_*>
        # one stage outside the arenas (CI Training: `dish STAGE_CITRAINING`), for
        # `pd_import` to rebuild a level over: its bg, its tiles, and
        # `textures/index.json` for the textures it uses (no pads or setup)
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

PADFLAG_HASBBOXDATA = 0x0200  # constants.h:3330

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
    extra = {k: d[k] for k in ("stage", "units", "env", "rooms", "portals", "bgcmds", "lights", "section2_textures")}
    nbytes, tris = pd_models.write_model(model, "bg", filenum, filename, out("stages", stem), EXPORTER, extra)
    return {"bg_tris": tris, "bg_bytes": nbytes, "bg_textures": len(textures)}


# ---------------------------------------------------------------------------
# Tiles, pads
# ---------------------------------------------------------------------------


def room_numbers(names: list[str], path: str) -> dict[str, int]:
    """The rooms of a tiles file by number. The asset tool names them
    `ROOM_<STAGE>_<nnnn>` in order, but a room the game's code names keeps its
    name (CI's `ROOM_DISH_FIRINGRANGE`, 0x0a): that one's number is its place
    in the file, checked against the numbered ones."""
    nums = {}
    for i, name in enumerate(names):
        try:
            n = id_suffix(name)
        except ValueError:
            n = i
        if n != i:
            raise SystemExit(f"{decomp_rel(path)}: {name} is at {i}")
        nums[name] = n
    return nums


def export_tiles(stem: str) -> dict:
    path = asset("tiles", f"{stem}.json")
    with open(path, encoding="utf-8") as fh:
        rooms = json.load(fh)["rooms"]
    num = room_numbers(list(rooms), path)
    out_tiles = []
    for name in sorted(rooms, key=num.get):
        for t in rooms[name]:
            flags = 0
            for key, bit in GEOFLAGS.items():
                if t.get(key):
                    flags |= bit
            verts = [[v["x"], v["y"], v["z"]] for v in t["vertices"]]
            if len(verts) < 3:
                raise SystemExit(f"{name}: a tile with {len(verts)} vertices")
            out_tiles.append({"room": num[name], "flags": flags,
                              "floortype": FLOORTYPES[t.get("floortype", "default")],
                              "floorcol": t.get("floorcolour", 0), "verts": verts})
    write_json(out("stages", stem, "tiles.json"), {
        "format": "pd-tiles/1", "source": decomp_rel(path), "exporter": EXPORTER,
        "rooms": sorted(num.values()), "tiles": out_tiles,
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
        # mkpads (tools/assetmgr/mkpads:185): a box other than ±100 is stored,
        # and flagged; pad_unpack gives ±100 without it (pad.c:82).
        box = (p["xmin"], p["xmax"], p["ymin"], p["ymax"], p["zmin"], p["zmax"])
        if box != (-100, 100, -100, 100, -100, 100):
            flags |= PADFLAG_HASBBOXDATA
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


#: `struct stagetableentry` (types.h:3075), the fields of a `g_Stages` row.
STAGETABLE_FIELDS = [
    "id", "light_type", "light_alpha", "light_width", "light_height", "unk06",
    "bgfileid", "tilefileid", "padsfileid", "setupfileid", "mpsetupfileid",
    "unk14", "unk18", "unk1c", "unk20", "unk22", "unk23", "unk24", "unk28",
    "unk2c", "eraserpropdist", "unk30", "unk34",
]


def stage_row(stage_name: str) -> dict:
    """The stage's `g_Stages` row (`stagetable.c`), numbers evaluated, file and
    texture names kept as their symbols."""
    path = src("game", "stagetable.c")
    text = gen.read(path)
    m = re.search(rf"/\*0x[0-9a-f]+\*/\s*({stage_name}\s*,[^\n]*)", text)
    if not m:
        raise SystemExit(f"{decomp_rel(path)}: no g_Stages row for {stage_name}")
    args = [a.strip() for a in m.group(1).rstrip().rstrip(",").split(",")]
    if len(args) != len(STAGETABLE_FIELDS):
        raise SystemExit(f"{decomp_rel(path)}: {stage_name}'s row has {len(args)} fields")
    c = gen.Consts()
    c.load_header(src("include", "constants.h"))
    row = {}
    for k, a in zip(STAGETABLE_FIELDS, args):
        try:
            row[k] = int(a, 0)
        except ValueError:
            try:
                row[k] = float(a.rstrip("f"))
            except ValueError:
                row[k] = a
    return row


#: The setup macros that place a model (their `model` argument).
MODEL_MACROS = ("stdobject", "door", "lift", "glass", "tinted_glass", "hover_prop", "weapon", "ammocratemulti")


def setup_models(c, macros: tuple[str, ...] = MODEL_MACROS) -> list[int]:
    """Every MODEL_ number an arena's MP setup places with one of `macros`,
    sorted (`pd_models.py` exports them)."""
    params = macro_params("intro.h", "props.h")
    found = set()
    for stem in pd_bg.STAGES:
        text = gen.preprocess(gen.read(src("setups", f"mp_setup{stem}.c")))
        m = re.search(r"\bprops\[\]\s*=\s*\{(.*?)\n\};", text, re.S)
        for macro, args in calls(m.group(1)):
            if macro in macros:
                found.add(c.eval(args[params[macro].index("model")]))
    return sorted(found)


#: The commands an MP setup's background AI lists use (`include/commands.h`),
#: by their parameter names. Anything else is an error: the world would not run it.
BGAI_COMMANDS = {
    "activate_lift": ["liftid", "object"],
    "set_ailist": ["chr", "ailist"],
    "set_wind_speed": ["speed"],
    "mp_init_simulants": [], "rebuild_teams": [], "rebuild_squadrons": [],
}


def bg_ailists(text: str, path: str, value) -> list[dict]:
    """The setup's background AI lists (`ailists[]` ids from 0x1000, each run
    by a BG chr, `chraireset.c:53`) as `{id, cmds: [{type, <param>: value}]}`."""
    m = re.search(r"\bailists\[\]\s*=\s*\{(.*?)\n\};", text, re.S)
    if not m:
        raise SystemExit(f"{decomp_rel(path)}: no ailists[]")
    lists = []
    for fm in re.finditer(r"\{\s*(\w+)\s*,\s*(0x[0-9a-fA-F]+|\d+)\s*\}", m.group(1)):
        func, lid = fm.group(1), int(fm.group(2), 0)
        if func == "NULL" or lid < 0x1000:
            continue
        bm = re.search(rf"\bu8\s+{func}\[\]\s*=\s*\{{(.*?)\n\}};", text, re.S)
        if not bm:
            raise SystemExit(f"{decomp_rel(path)}: no {func}[]")
        cmds = []
        for macro, args in calls(bm.group(1)):
            if macro == "endlist":
                break
            names = BGAI_COMMANDS.get(macro)
            if names is None or len(names) != len(args):
                raise SystemExit(f"{decomp_rel(path)}: {func}: {macro}({', '.join(args)}) is not a BG AI command the world runs")
            cmds.append({"type": macro, **{k: value(a) for k, a in zip(names, args)}})
        lists.append({"id": lid, "cmds": cmds})
    return lists


def export_setup(stem: str, stage_name: str) -> dict:
    path = src("setups", f"mp_setup{stem}.c")
    # Felicity's and Grid's props have `#if VERSION >= VERSION_NTSC_1_0` rows.
    text = gen.preprocess(gen.read(path))
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
        "stage": {"name": stage_name, "num": c.eval(stage_name), "code": stem, "table": stage_row(stage_name)},
        "intro": intro, "props": props, "bgai": bg_ailists(text, path, value),
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


def export_base(stem: str, stage_name: str) -> dict:
    """One stage outside `pd_bg.STAGES`, for `pd_import`: its BG and tiles, and
    the index of the textures it uses (their surface types), which a full run's
    pool index would hold."""
    pd_bg.STAGES.setdefault(stem, (stage_name, f"bg_{stem}.seg"))
    pool = pd_models.TexturePool()
    c = {}
    c.update(export_bg(stem, pool))
    c.update(export_tiles(stem))
    write_json(out("textures", "index.json"), {f"{k:04x}": v for k, v in sorted(pool.entries.items())})
    print(f"pd_stage {stem}: {json.dumps(c)}")
    return c


def main() -> int:
    if sys.argv[1:2] == ["--base"]:
        if len(sys.argv) != 4:
            raise SystemExit("pd_stage.py --base <code> <STAGE_*>")
        export_base(sys.argv[2], sys.argv[3])
        return 0
    pool = pd_models.TexturePool()
    # The pool's index is rebuilt by build_assets.py; alone, this run only
    # (re)writes the stages and the textures they use.
    export_all(pool)
    return 0


if __name__ == "__main__":
    sys.exit(main())
