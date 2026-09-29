#!/usr/bin/env python3
"""Export every model file the Combat Simulator draws, in the one model format,
and the global texture pool they (and the menus, effects and BG) sample.

Every model goes through `pd_fpgun.export_model`: the display-list interpreter
that records, per triangle batch, the N64 state that drew it (combiner, lighting
and texgen, tile, blender, z, cull). The node tree keeps every node type PD walks,
in PD's depth-first order: CHRINFO, POSITION (with the `MODELNODETYPE_0100`/`0200`
elbow and knee helper slots in `mtx[1]`/`mtx[2]` and the flags in `flags`),
POSITIONHELD, TOGGLE, DISTANCE, HEADSPOT, BBOX part boxes (`hitpart` + box, which
`model_test_for_hit` reads), CHRGUNFIRE, STARGUNFIRE, GUNDL and DL.

# The format (read by `pd_core::model`)

`models/<stem>.json`, UTF-8:

    format        "pd-model/1"
    name, stem    the modeldef's name and the file stem
    filenum, file PD's FILE_* number and name (`include/files.h`)
    source        the decomp file it came from
    skel          modeldef.skel (0x09 chr body, 0x0d head, 0x2a hudpiece, ...)
    nummatrices   modeldef.nummatrices
    scale         modeldef.scale: the model's radius in its own units, what
                  `pos_is_onscreen` and the LOD code multiply by `model->scale`
    nodes[]       {type, parent, partnum?, ...type fields}; parent is an index
    parts         {MODELPART_* number: node index}
    materials[]   interpreted draw states; `texture.id` is a pool number, or
                  0x10000 | texconfig index for a texture stored in the model
    textures      {texture id: {file, w, h, cfg_w, cfg_h, levels, source}};
                  `file` is relative to the asset root
    batches[]     {node, material, nverts, nidx}, in draw order
    vertex        the byte layout of one vertex in the .bin

`models/<stem>.bin`, little-endian, the batches back to back: `nverts` vertices
of 28 bytes (`f32 x,y,z; u16 mtx; f32 u,v; u8 c0,c1,c2,c3; u8 flags; u8 pad`,
flags 1 = lit, 2 = texgen; c0..c2 are a normal when lit), then `nidx` u16
indices into that batch's vertices.

`models/index.json`: {stem: {filenum, file, kind, source, tris, [modelnum, statescale]}} (the
`g_ModelStates` row naming the file, if any).

# The texture pool

`textures/<num>.png` (RGBA8, level 0, in N64 display space: no gamma) for every
pool texture a model references, plus the ones the menus, the gun effects and the
stage BGs sample (added by `pd_stage.py`). `textures/index.json`: {"<num>": {w, h, format, codec,
numcolours, numlods, hasloddata, soundsurfacetype, surfacetype}} (the last two
from `g_Textures`: what a shot hitting it sounds like and leaves). PD rebuilds mip levels at load
(`tex_shrink_*`) unless `hasloddata`; per-use texconfig levels are in each model's
`textures` entry. Textures stored inside a model file (a51guard, dd_shock, elvis,
the casings) are model-local: `models/tex/<stem>_<index>.png`.

Usage:
    python tools/pd-assets/pd_models.py              # into assets/ (pd_paths.OUT)
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
import pd_gltf  # noqa: E402
import pd_menu_gen as gen  # noqa: E402
import pd_model  # noqa: E402
import pd_tex  # noqa: E402
from pd_paths import asset, decomp_rel, out, out_rel, src  # noqa: E402

FORMAT = "pd-model/1"
VERTEX = struct.Struct("<fffHffBBBBBx")
VERTEX_LAYOUT = {
    "size": VERTEX.size,
    "fields": ["x:f32", "y:f32", "z:f32", "mtx:u16", "u:f32", "v:f32",
               "c0:u8", "c1:u8", "c2:u8", "c3:u8", "flags:u8", "pad:u8"],
    "flags": {"lit": 1, "texgen": 2},
}

#: Textures the menus draw from C rather than through a model: `menugfx.c`'s
#: menu rays and background (TEX_GENERAL_*), as the menu spike loads them
#: (old repo `pd_menu/mod.rs:147`).
MENU_TEXTURES = [0x0001, 0x01E5, 0x084E, 0x0858, 0x08F4, 0x0C9A]

#: Textures the gun code draws directly: beams and lasers (`beam.c`, old repo
#: `pd_guns/fx.rs:76`), sparks, wall hits (`wallhit.c`, `fx.rs:523`), smoke
#: (`smoke.c`, 0x002a), explosions (`explosions.c`, 0x001e..), the sight and HUD
#: pieces. The set the gun spike exported into its `fx/`.
FX_TEXTURES = [
    0x0003, 0x0004, 0x0005, 0x0006, 0x0007, 0x0008, 0x0009, 0x000A, 0x000B, 0x000E,
    0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F, 0x0020, 0x0021, 0x0022, 0x0023,
    0x0024, 0x0025, 0x0026, 0x0027, 0x0028, 0x0029, 0x002A, 0x002B, 0x002C, 0x002D,
    0x002E, 0x002F, 0x0030, 0x0031, 0x0032, 0x0033, 0x0034, 0x0035, 0x0036, 0x0037,
    0x0038, 0x0039, 0x003A, 0x003B, 0x063B, 0x0854, 0x0855, 0x0856, 0x0859, 0x085A,
    0x08F0, 0x0B53, 0x0C27, 0x0C28, 0x0C32, 0x0C97, 0x0DA5,
    # g_TcSkyWaterConfigs[TEX_ENV_00] (textureconfig.c:233): the clouds.
    0x0013,
    # g_TcShieldConfigs[TEX_SHIELD_00] (textureconfig.c:125): the shield's
    # shimmer; g_TcRadarConfigs[TEX_RADAR_BG] (:324): the radar's disc.
    0x000D, 0x003C,
]

# ---------------------------------------------------------------------------
# FILE_* numbers
# ---------------------------------------------------------------------------


def model_states() -> dict[str, tuple[int, int]]:
    """FILE_ name -> (MODEL_ number, scale) from `g_ModelStates`
    (`modeldata/general.c`), the first row naming the file. An object's model
    scale is `scale / 4096` (`obj_init`, `propobj.c:2098`)."""
    text = gen.read(src("game", "modeldata", "general.c"))
    states: dict[str, tuple[int, int]] = {}
    for m in re.finditer(r"/\*0x([0-9a-f]+)\*/\s*\{\s*NULL,\s*(FILE_\w+),\s*(\w+)\s*\}", text):
        states.setdefault(m.group(2), (int(m.group(1), 16), int(m.group(3), 0)))
    return states


def model_state_rows() -> dict[int, tuple[str, int]]:
    """MODEL_ number -> (FILE_ name, scale): every `g_ModelStates` row
    (`modeldata/general.c`)."""
    text = gen.read(src("game", "modeldata", "general.c"))
    return {int(m.group(1), 16): (m.group(2), int(m.group(3), 0))
            for m in re.finditer(r"/\*0x([0-9a-f]+)\*/\s*\{\s*NULL,\s*(FILE_\w+),\s*(\w+)\s*\}", text)}


def file_table() -> dict[str, tuple[int, str]]:
    """ROM path (e.g. "chrs/dark_combat.bin") -> (FILE number, FILE_ name).

    `files/list.c` gives the numbered file names ("Cdark_combatZ": C chrs, G guns,
    P props; a trailing Z is the compressed flag); `include/files.h` names them."""
    names = gen.file_names()
    table: dict[str, tuple[int, str]] = {}
    text = gen.read(asset("files", "list.c"))
    for m in re.finditer(r"/\*0x([0-9a-f]+)\*/\s*\"([^\"]+)\"", text):
        num, name = int(m.group(1), 16), m.group(2)
        if "/" in name:
            continue
        kind, stem = name[0], name[1:]
        if stem.endswith("Z"):
            stem = stem[:-1]
        folder = {"C": "chrs", "G": "guns", "P": "props"}.get(kind)
        if folder:
            table[f"{folder}/{stem.lower()}.bin"] = (num, "FILE_" + names.get(num, f"{num:04X}"))
    return table


def chr_filenums(c: gen.Consts) -> set[int]:
    """Every body and head the Combat Simulator can put on a player or simulant
    (`g_MpBodies`, `g_MpHeads`, `g_MpBeauHeads`, `g_MpMaleHeads`, `g_MpFemaleHeads`
    in mplayer.c, through `g_HeadsAndBodies[].filenum` in modeldata/robot.c), plus
    the menus' hudpiece (FILE_GHUDPIECE)."""
    mp = gen.preprocess(gen.strip_comments(gen.read(src("game", "mplayer", "mplayer.c"))))
    robot = gen.preprocess(gen.strip_comments(gen.read(src("game", "modeldata", "robot.c"))))
    hb = gen.entries(gen.find_initialisers(robot, "struct headorbody")["g_HeadsAndBodies"])
    nums = set()
    for r in gen.entries(gen.find_initialisers(mp, "struct mpbody")["g_MpBodies"]):
        nums.add(c.eval(hb[c.eval(r[0])][5]))
        head = c.eval(r[2])
        if head != 1000:  # 1000: the body keeps its built-in head
            nums.add(c.eval(hb[head][5]))
    for table in ("g_MpHeads", "g_MpBeauHeads"):
        for r in gen.entries(gen.find_initialisers(mp, "struct mphead")[table]):
            nums.add(c.eval(hb[c.eval(r[0])][5]))
    for name in ("g_MpMaleHeads", "g_MpFemaleHeads"):
        for v in gen.split_top(gen.find_initialisers(mp, "u32")[name]):
            nums.add(c.eval(hb[c.eval(v)][5]))
    nums.add(c.eval("FILE_GHUDPIECE"))
    return nums


def consts() -> gen.Consts:
    c = gen.Consts()
    for h in ("constants.h", "files.h", "sfx.h"):
        c.load_header(src("include", h))
    for js in ("sequences.json", "animations.json"):
        rows = json.load(open(asset(js), encoding="utf-8"))
        for i, r in enumerate(rows):
            c.vals.setdefault(r["id"], i)
    return c


#: The scenarios' own objects (`mplayer/scenarios/*.inc`).
SCENARIO_MODELS = ("MODEL_CHRBRIEFCASE", "MODEL_CHRDATATHIEF", "MODEL_GOODPC")


def model_list(weapons: dict, c: gen.Consts) -> list[tuple[str, str]]:
    """(ROM path, kind) for every model to export, in a fixed order."""
    files: list[tuple[str, str]] = []

    def add(rel: str, kind: str) -> None:
        if rel and all(rel != f for f, _ in files):
            files.append((rel, kind))

    for w in weapons["weapons"]:
        add((w.get("assets") or {}).get("fp_model"), "gun")
    for h in pd_fpgun.HAND_FILES:
        add(f"guns/{h}", "hand")
    # FILE_GCOMBATHANDSLOD: the unarmed fists are a weapon model (bondgun.c:3865).
    add("guns/combathandslod.bin", "gun")
    # g_CartFileNums (bondgun.c:167): the ejected casings, by ammo casingeject.
    for f in ("cartridge", "cartrifle", "cartblue", "cartshell"):
        add(f"guns/{f}.bin", "casing")
    # g_MpWeapons[].model through g_ModelStates: the third-person gun a chr holds
    # (and the pickup on a weapon pad).
    for w in weapons["weapons"]:
        add((w.get("assets") or {}).get("tp_model"), "held")
    for f in pd_fpgun.PROP_FILES:
        add(f"props/{f}", "prop")
    # The MP ammo crate beside each weapon pad (MODEL_MULTI_AMMO_CRATE,
    # modeldata/general.c:599; ammocratemulti() in the setups).
    add("props/multi_ammo_crate.bin", "prop")
    table = file_table()
    by_num = {num: rel for rel, (num, _) in table.items()}
    # Every arena's setup objects (doors, lifts, glass, crates, hover props),
    # through g_ModelStates (general.c) by their `model` argument.
    import pd_stage  # noqa: PLC0415 (pd_stage imports this module)
    by_name = {name: rel for rel, (_, name) in table.items()}
    rows = model_state_rows()
    for modelnum in pd_stage.setup_models(c):
        fname = rows[modelnum][0]
        if fname not in by_name:
            raise SystemExit(f"MODEL {modelnum:#x} is {fname}, which is not in files/list.c")
        add(by_name[fname], "prop")
    # The scenarios' objects (M10): the briefcase and the data uplink lying on
    # the ground (htb_create_token, htb_create_uplink, ctc_init_props) and
    # Hacker Central's terminal (htm_init_props' scenario_create_obj).
    for name in SCENARIO_MODELS:
        fname = rows[c.eval(name)][0]
        add(by_name[fname], "prop")
    for n in sorted(chr_filenums(c)):
        rel = by_num.get(n)
        if rel is None:
            raise SystemExit(f"FILE {n:#x} is not in files/list.c")
        add(rel, "hud" if rel == "guns/hudpiece.bin" else "chr")
    return files


# ---------------------------------------------------------------------------
# The texture pool
# ---------------------------------------------------------------------------


_SURFACE_TYPES: list[tuple[int, int]] | None = None


def texture_surface_types() -> list[tuple[int, int]]:
    """`g_Textures[num]`'s `soundsurfacetype` and `surfacetype` (`types.h:4842`),
    from the extract's `textures.json`: `flag00` is the byte's high nibble, the
    sound one (`tools/assetmgr/mktextures:31`)."""
    global _SURFACE_TYPES
    if _SURFACE_TYPES is None:
        with open(asset("textures.json"), encoding="utf-8") as fh:
            rows = json.load(fh)
        _SURFACE_TYPES = [(r["flag00"] & 0x0f, r["surfacetype"] & 0x0f) for r in rows]
    return _SURFACE_TYPES


class TexturePool:
    """Writes `textures/<num>.png` once per pool texture and model-local textures
    to `models/tex/`, and collects `textures/index.json`."""

    def __init__(self) -> None:
        self.entries: dict[int, dict] = {}
        os.makedirs(out("textures"), exist_ok=True)
        os.makedirs(out("models", "tex"), exist_ok=True)

    def pool(self, texnum: int) -> dict:
        """Decode and write pool texture `texnum` (once); its index entry."""
        e = self.entries.get(texnum)
        if e is not None:
            return e
        path = asset("textures", f"{texnum:04x}.bin")
        with open(path, "rb") as fh:
            data = fh.read()
        t = pd_tex.decode(data)
        with open(out("textures", f"{texnum:04x}.png"), "wb") as fh:
            fh.write(pd_gltf.png_bytes(t.width, t.height, t.rgba))
        sound, surface = texture_surface_types()[texnum]
        e = {
            "w": t.width, "h": t.height, "format": t.format_name,
            "codec": "zlib" if data[0] & 0x40 else "non-zlib",
            "numcolours": t.numcolours, "numlods": t.numlods, "hasloddata": bool(t.hasloddata),
            "soundsurfacetype": sound, "surfacetype": surface,
        }
        self.entries[texnum] = e
        return e

    def sink(self, m: pd_model.ModelDef, cfg: pd_gltf.TexConfig, texid: int, stem: str) -> dict:
        """`export_model`'s texture sink."""
        if cfg.inline:
            w, h = cfg.width, cfg.height
            try:
                rgba = pd_gltf.decode_inline_texture(m, cfg)
            except SystemExit as e:  # the texture runs past the end of the file
                raise pd_tex.UnsupportedTexture(str(e)) from None
            path = out("models", "tex", f"{stem}_{cfg.index:03x}.png")
            with open(path, "wb") as fh:
                fh.write(pd_gltf.png_bytes(w, h, rgba))
        else:
            e = self.pool(cfg.texnum)
            w, h = e["w"], e["h"]
            path = out("textures", f"{cfg.texnum:04x}.png")
            if (w, h) != (cfg.width, cfg.height):
                print(f"  NOTE: texture {cfg.texnum:#x} is {w}x{h} but {m.name}'s texconfig says "
                      f"{cfg.width}x{cfg.height}", file=sys.stderr)
        return {"file": out_rel(path), "w": w, "h": h, "cfg_w": cfg.width, "cfg_h": cfg.height,
                "levels": cfg.levels, "source": "pd"}

    def write_index(self) -> None:
        with open(out("textures", "index.json"), "w", encoding="utf-8", newline="\n") as fh:
            json.dump({f"{n:04x}": self.entries[n] for n in sorted(self.entries)}, fh, indent=1)
            fh.write("\n")


# ---------------------------------------------------------------------------
# Models
# ---------------------------------------------------------------------------


def write_model(d: dict, stem: str, filenum: int, filename: str, dirpath: str | None = None,
                exporter: str = "tools/pd-assets/pd_models.py", extra: dict | None = None) -> tuple[int, int]:
    """Split the exporter's dict into `<stem>.json` + `<stem>.bin` in `dirpath`
    (default `models/`), with `extra` keys added to the header; (bytes, tris)."""
    dirpath = dirpath or out("models")
    blob = bytearray()
    heads = []
    for b in d["batches"]:
        if len(b["verts"]) > 0xFFFF:
            raise SystemExit(f"{stem}: a batch has {len(b['verts'])} vertices (u16 indices)")
        for x, y, z, mtx, u, v, c0, c1, c2, c3, flags in b["verts"]:
            blob += VERTEX.pack(x, y, z, int(mtx), u, v, int(c0), int(c1), int(c2), int(c3), int(flags))
        blob += struct.pack(f"<{len(b['indices'])}H", *b["indices"])
        # Anything else a batch carries (the BG's `leaf`, `cidx`, `dyntex`,
        # `st`) rides in its header entry.
        heads.append({"node": b["node"], "material": b["material"],
                      "nverts": len(b["verts"]), "nidx": len(b["indices"]),
                      **{k: v for k, v in b.items() if k not in ("node", "material", "verts", "indices")}})
    head = {
        "format": FORMAT,
        "name": d["name"],
        "stem": stem,
        "filenum": filenum,
        "file": filename,
        "source": d["source"],
        "exporter": exporter,
        "skel": d["skel"],
        "nummatrices": d["nummatrices"],
        # The stage BG has no modeldef: 1.
        "scale": d.get("scale", 1.0),
        "nodes": d["nodes"],
        "parts": d["parts"],
        "materials": d["materials"],
        "textures": d["textures"],
        "batches": heads,
        "vertex": VERTEX_LAYOUT,
        **(extra or {}),
    }
    os.makedirs(dirpath, exist_ok=True)
    with open(os.path.join(dirpath, stem + ".json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(head, fh, separators=(",", ":"))
        fh.write("\n")
    with open(os.path.join(dirpath, stem + ".bin"), "wb") as fh:
        fh.write(bytes(blob))
    return len(blob), sum(len(b["indices"]) // 3 for b in d["batches"])


def export_bodies(c: gen.Consts, table: dict[str, tuple[int, str]]) -> int:
    """`data/bodies.json`: `g_HeadsAndBodies` (modeldata/robot.c:64), indexed by
    `BODY_*`/`HEAD_*` number, with each row's model stem. `body.c:170` builds a
    chr's model scale from `scale * 0.1` and its animscale from `animscale`."""
    robot = gen.preprocess(gen.strip_comments(gen.read(src("game", "modeldata", "robot.c"))))
    stem_of = {num: os.path.splitext(os.path.basename(rel))[0] for rel, (num, _) in table.items()}
    names = gen.file_names()
    rows = []
    for i, r in enumerate(gen.entries(gen.find_initialisers(robot, "struct headorbody")["g_HeadsAndBodies"])):
        filenum, hand = c.eval(r[5]), c.eval(r[9])
        rows.append({
            "num": i, "ismale": c.eval(r[0]) != 0, "unk00_01": c.eval(r[1]) != 0,
            "canvaryheight": c.eval(r[2]) != 0, "type": c.eval(r[3]), "height": c.eval(r[4]),
            "filenum": filenum, "file": "FILE_" + names.get(filenum, "?"), "stem": stem_of.get(filenum),
            "scale": float(r[6].strip().rstrip("f")), "animscale": float(r[7].strip().rstrip("f")),
            "handfilenum": hand, "hand": stem_of.get(hand),
        })
    # g_MpBodies / g_MpHeads (mplayer.c:1835, :1674): the menu's choices, as
    # g_HeadsAndBodies numbers. A body's head 1000 means a random one from
    # g_MpMaleHeads / g_MpFemaleHeads (mp_get_mpheadnum_by_mpbodynum, :2560).
    mp = gen.preprocess(gen.strip_comments(gen.read(src("game", "mplayer", "mplayer.c"))))
    maleheads = [c.eval(v) for v in gen.split_top(gen.find_initialisers(mp, "u32")["g_MpMaleHeads"])]
    femaleheads = [c.eval(v) for v in gen.split_top(gen.find_initialisers(mp, "u32")["g_MpFemaleHeads"])]
    mpbodies = [{"bodynum": c.eval(r[0]), "headnum": c.eval(r[2]), "requirefeature": c.eval(r[3])}
                for r in gen.entries(gen.find_initialisers(mp, "struct mpbody")["g_MpBodies"])]
    mpheads = [{"headnum": c.eval(r[0]), "requirefeature": c.eval(r[1])}
               for r in gen.entries(gen.find_initialisers(mp, "struct mphead")["g_MpHeads"])]
    os.makedirs(out("data"), exist_ok=True)
    with open(out("data", "bodies.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump({"source": "pd-decomp/src/game/modeldata/robot.c g_HeadsAndBodies; "
                             "game/mplayer/mplayer.c g_MpBodies, g_MpHeads",
                   "rows": rows, "mpbodies": mpbodies, "mpheads": mpheads,
                   "maleheads": maleheads, "femaleheads": femaleheads}, fh, indent=1)
        fh.write("\n")
    return len(rows)


def export_all(weapons: dict) -> tuple[dict, "TexturePool"]:
    """Export the models and start the pool. Returns counts for MANIFEST.json and
    the pool, which the stage exporter adds its BG textures to before the caller
    writes `textures/index.json` (`TexturePool.write_index`)."""
    c = consts()
    table = file_table()
    states = model_states()
    pool = TexturePool()
    index: dict[str, dict] = {}
    warned = 0
    total = 0
    import pd_stage  # noqa: PLC0415 (pd_stage imports this module)
    door_models = set(pd_stage.setup_models(c, ("door",)))
    for rel, kind in model_list(weapons, c):
        path = asset("files", *rel.split("/"))
        if not os.path.exists(path):
            raise SystemExit(f"missing {decomp_rel(path)}")
        stem = os.path.splitext(os.path.basename(rel))[0]
        filenum, filename = table[rel]
        dlverts = filename in states and states[filename][0] in door_models
        d, warnings = pd_fpgun.export_model(path, None, tex_sink=pool.sink, dlverts=dlverts)
        nbytes, tris = write_model(d, stem, filenum, filename)
        total += nbytes
        index[stem] = {"filenum": filenum, "file": filename, "kind": kind, "source": decomp_rel(path), "tris": tris}
        if filename in states:
            index[stem]["modelnum"], index[stem]["statescale"] = states[filename]
        for w in warnings:
            warned += 1
            print(f"  {stem}: {w}", file=sys.stderr)
    for n in MENU_TEXTURES + FX_TEXTURES:
        pool.pool(n)
    nbodies = export_bodies(c, table)
    with open(out("models", "index.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump({k: index[k] for k in sorted(index)}, fh, indent=1)
        fh.write("\n")
    nlocal = len(os.listdir(out("models", "tex")))
    print(f"pd_models: {len(index)} models ({total / 1e6:.1f} MB), {len(pool.entries)} pool textures, "
          f"{nlocal} model-local textures, {warned} warnings")
    return {"models": len(index), "model_textures": nlocal, "headsandbodies": nbodies}, pool


def main() -> int:
    import pd_stage  # noqa: PLC0415 (the stage exporter imports this module)

    weapons, _ = pd_fpgun.build_weapons()
    _, pool = export_all(weapons)
    pd_stage.export_all(pool)
    pool.write_index()
    return 0


if __name__ == "__main__":
    sys.exit(main())
