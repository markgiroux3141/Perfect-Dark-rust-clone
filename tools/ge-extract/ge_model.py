"""GoldenEye prop models (the doors) into the one model format.

A GE model file is a PD model file without its header: the game keeps GE's
`ModelFileHeader` (skeleton, switches, matrices, radius, texture count) in its
own data (`pobjdata.c`, from `assets/obseg/prop/<name>/ModelFileHeader.inc.c`),
and the file starts with `Switches[numswitches]` (u32 node pointers), then the
texture table (12-byte entries laid out as PD's `textureconfig`, the image a
global GE image number), then the root node (`load_object_fill_header`,
`objecthandler_2.c:89-107`). Pointers are segment 0x05, offsets into the file
(`modelPromoteNodeOffsetsToPointers`, `model.c:5681`).

Nodes are PD's 0x18-byte records, and the node types a door uses are PD's by
number (`bondconstants.h:1876`): 0x02 GROUP (PD's POSITION), 0x0a BBOX, 0x12
SWITCH (TOGGLE), 0x18 DLCOLLISION (PD's DL, laid out differently: `{primary,
secondary, vertices, s16 numvertices, s16 numcollisionvertices, collision
vertices, point usage, s16 modeltype, ...}`, `bondtypes.h:1269`). Vertices are
fast3d's 16-byte `Vtx_t`, the display lists PD's dialect, so they run through
`pd_fpgun.Interp` as a PD model's do; GE's collision vertices have no PD
counterpart (a PD door collides by its box) and are dropped.

The skeletons: GE's `standard_object` is PD's `SKEL_BASIC`, its `door` PD's
`SKEL_WINDOWEDDOOR` (the same four parts: bbox, the glass's switch, its bbox,
its DL: `propobj.c:5839, 9447` against PD's `constants.h:2578`). A switch is
its part: GE's `Switches[i]` is part `i`.
"""

from __future__ import annotations

import os
import struct
import sys
import types

from ge_gbi import GeDialect
from ge_rom import PD_ASSETS_TOOLS, Ge, GeError
from ge_tex import Images

sys.path.insert(0, PD_ASSETS_TOOLS)
import pd_fpgun  # noqa: E402
import pd_models  # noqa: E402

SEG = 0x05000000
GROUP, BBOX, SWITCH, DLCOLLISION = 0x02, 0x0A, 0x12, 0x18
SKELS = {"standard_object": 0x02, "door": 0x10}  # PD's SKEL_BASIC, SKEL_WINDOWEDDOOR (constants.h:3734)

#: Custom models' numbers: past PD's `MODEL_*` (0..0x1bd), `pd_core::assets::CUSTOM_MODELNUMS`.
CUSTOM_MODELNUM_BASE = 0x1000
#: Their file numbers: past PD's `FILE_*`.
CUSTOM_FILENUM_BASE = 0x10000


class GeInterp(GeDialect, pd_fpgun.Interp):
    """PD's model interpreter over a GE model's display lists (`ge_gbi`)."""

    def __init__(self, data: bytes, images: Images, cfgs: dict, name: str) -> None:
        super().__init__(types.SimpleNamespace(data=data, name=name), cfgs, name)
        self.images = images


def off(p: int, n: int) -> int | None:
    return p - SEG if SEG <= p < SEG + n else None


def export(ge: Ge, images: Images, propnum: int) -> tuple[str, dict, dict, list[str]]:
    """Prop `propnum`'s model as `pd_models.write_model` takes it: (stem, model,
    index entry, warnings)."""
    name = ge.props()[propnum]
    fname, scale = ge.prop_record(name)
    hdr = ge.model_header(name)
    d = ge.file(fname)
    if hdr["skeleton"] not in SKELS:
        raise GeError(f"{fname}: skeleton {hdr['skeleton']} has no PD counterpart here")
    switches = [struct.unpack_from(">I", d, 4 * i)[0] for i in range(hdr["numswitches"])]
    root = 4 * hdr["numswitches"] + 12 * hdr["numtextures"]

    def node(o: int) -> tuple[int, int, int, int, int]:
        t, data, parent, nxt, _prev, child = struct.unpack_from(">HxxIIIII", d, o)
        return t & 0xFF, data, parent, nxt, child

    # Depth first, as `model_render` walks it.
    order: list[int] = []
    o: int | None = root
    while o is not None:
        order.append(o)
        _t, _data, parent, nxt, child = node(o)
        if off(child, len(d)) is not None:
            o = off(child, len(d))
            continue
        o = None
        cur_next, cur_parent = nxt, parent
        while True:
            if off(cur_next, len(d)) is not None:
                o = off(cur_next, len(d))
                break
            if off(cur_parent, len(d)) is None:
                break
            _t, _dd, cur_parent, cur_next, _c = node(off(cur_parent, len(d)))
    index = {o: i for i, o in enumerate(order)}

    cfgs = {}
    for i in range(hdr["numtextures"]):
        (image,) = struct.unpack_from(">I", d, 4 * hdr["numswitches"] + 12 * i)
        cfgs[image] = images.config(image)
    interp = GeInterp(d, images, cfgs, fname)
    nodes = []
    dl_nodes = set()
    mtx = 0
    for i, o in enumerate(order):
        t, data, parent, _nxt, _child = node(o)
        ro = off(data, len(d))
        rec: dict = {"parent": index.get(off(parent, len(d)), -1) if off(parent, len(d)) is not None else -1}
        if t == GROUP and ro is not None:
            x, y, z, part, i0, i1, i2 = struct.unpack_from(">fffHhhh", d, ro)
            rec.update(type="position", pos=[x, y, z], animpart=part, mtx=[i0, i1, i2], flags=0)
            mtx = i0
        elif t == BBOX and ro is not None:
            hitpart, x0, x1, y0, y1, z0, z1 = struct.unpack_from(">iffffff", d, ro)
            rec.update(type="bbox", hitpart=hitpart, bbox=[x0, x1, y0, y1, z0, z1])
        elif t == SWITCH and ro is not None:
            tgt, rw = struct.unpack_from(">IH", d, ro)
            rec.update(type="toggle", rw=rw, target=index.get(off(tgt, len(d)), -1) if off(tgt, len(d)) is not None else -1)
        elif t == DLCOLLISION and ro is not None:
            pri, sec, vtx, _nv = struct.unpack_from(">IIIh", d, ro)
            (rm,) = struct.unpack_from(">h", d, ro + 0x18)
            vb = off(vtx, len(d)) or 0
            gdls = [(g, x) for g, x in ((pri, False), (sec, True)) if g]
            dl_nodes.add(i)
            cull = interp.run_node(i, None, rm, gdls, {0x04: vb, 0x05: 0}, mtx)
            rec.update(type="dl", rendermode=rm)
            if cull is not None:
                rec["cull_exit"] = cull
        else:
            raise GeError(f"{fname}: node type {t:#x} at {o:#x} is not one a GE door uses")
        nodes.append(rec)
    parts = {str(p): index[off(s, len(d))] for p, s in enumerate(switches) if off(s, len(d)) in index}
    for p, n in parts.items():
        nodes[n]["partnum"] = int(p)

    batches = []
    for b in interp.batches:
        verts = [[*v["pos"], v["mtx"], round(v["uv"][0], 5), round(v["uv"][1], 5), *v["c"],
                  (1 if v["lit"] else 0) | (2 if v["texgen"] else 0)] for v in b["verts"]]
        batch = {"node": b["node"], "material": b["material"], "verts": verts, "indices": b["indices"]}
        # door_calc_texturemap's vertex table (pd_fpgun.export_model's `dlverts`).
        if b["node"] in dl_nodes and all(v["vsrc"] >= 0 for v in b["verts"]):
            batch["vsrc"] = [v["vsrc"] for v in b["verts"]]
            batch["st"] = [list(v["st_raw"]) for v in b["verts"]]
            batch["stscale"] = [list(v["texscale"]) for v in b["verts"]]
        batches.append(batch)
    used = sorted({m["texture"]["id"] & 0xFFFF for m in interp.materials if m["texture"]})
    stem = f"ge_{name}"
    model = {"name": stem, "source": f"ge007.u {fname} (ROM)", "nummatrices": hdr["nummatrices"],
             # PD's modeldef `scale` is the bounding radius, as GE's header's is.
             "scale": hdr["radius"], "skel": SKELS[hdr["skeleton"]], "nodes": nodes, "parts": parts,
             "materials": interp.materials, "batches": batches,
             "textures": {str(0x10000 | n): images.entry(n) for n in used}}
    entry = {"filenum": CUSTOM_FILENUM_BASE | propnum, "file": f"GE_{fname.upper()}", "kind": "prop",
             "source": model["source"], "tris": sum(len(b["indices"]) // 3 for b in batches),
             "modelnum": CUSTOM_MODELNUM_BASE + propnum,
             # PROPFILERECORD's scale is the prop's model scale, PD's g_ModelStates scale / 4096.
             "statescale": round(scale * 4096)}
    return stem, model, entry, sorted(set(interp.warnings))


def write(custom: str, stem: str, model: dict, entry: dict) -> None:
    """`<custom>/models/<stem>.json + .bin`, and its row in `<custom>/models/index.json`."""
    import json

    dirpath = os.path.join(custom, "models")
    pd_models.write_model(model, stem, entry["filenum"], entry["file"], dirpath, "tools/ge-extract/ge_model.py")
    path = os.path.join(dirpath, "index.json")
    index = {}
    if os.path.exists(path):
        with open(path, encoding="utf-8") as fh:
            index = json.load(fh)
    index[stem] = entry
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(dict(sorted(index.items())), fh, indent=1)
        fh.write("\n")
