"""GoldenEye's BG ("bg_<level>_all_p") into a PD stage's `bg.json` + `bg.bin`.

GE's BG is PD's ancestor, and the parts PD's game runs on are the same: rooms,
each a primary (opaque) and optional secondary (translucent) display list over
its own vertex table; portals between pairs of rooms; the same DL dialect
(fast3d with Rare's G_TRI4 and the C0 texture command). So a GE room becomes a
PD room one for one, each DL runs through `pd_fpgun.Interp` (the interpreter
PD's arenas and guns are exported through) with GE's 16-byte vertex, and the
result is written by `pd_models.write_model` exactly as `pd_stage.py` writes an
arena's.

The layout (ge-decomp, `src/game/bg.c`, `assets/obseg/bg/bg_all_p.h`):

* Header, five u32s: 0, the room table, the portal table, the global
  visibility commands, 0. Pointers are linked at 0x0F000000 (`bg_all_p.ld`),
  file offset = ptr - 0x0F000000; nothing is relocated per room.
* Room table, 24-byte rows `{point table, primary DL, secondary DL, f32 pos[3]}`,
  row 0 empty. `g_MaxNumRooms` counts rows while the primary DL is set
  (`bg.c:856`); rooms `1 .. g_MaxNumRooms - 1` are real (`bg.c:891`), the last
  row marks the end of the data. Each of a room's three blobs is its own 1172
  stream (`bgLoadRoomVtxData` `bg.c:2247`, `...PrimaryGdl` :2284,
  `...SecondaryGdl` :2349).
* Vertices are fast3d's `Vtx_t` (16 bytes, the baked colour inline), relative to
  the room's pos (`bgBuildRoomVtxBounds`, `bg.c:2862`); G_VTX addresses them in
  segment 0x0E (`bg.c:2688`).
* Portal table, 8-byte rows `{verts, u8 room1, u8 room2, u16 control}` to a null
  pointer (`bg.c:865`); verts `{u8 n, pad[3], f32 xyz[n]}`, absolute.
* Units: BG space is world x `levelscale` (`bgroomtrans.c:211-218`: the room
  matrix is `1 / levelscale`); stan tiles and setup pads are in the same space
  (`stan.c:331`, `prop.c:1352`). World units are centimetres (Bond's eye is
  `185 x perspective height - 10`, `bondview.c:1507`; PD's Joanna's 159).
"""

from __future__ import annotations

import struct
import sys
import types

from ge_gbi import GeDialect
from ge_rom import PD_ASSETS_TOOLS, Ge, GeError, inflate1172
from ge_tex import Images

sys.path.insert(0, PD_ASSETS_TOOLS)
import pd_bg  # noqa: E402
import pd_fpgun  # noqa: E402

SEG = 0x0F000000
SEG_VTX = 0x0E  # the room's vertices (`bg.c:2688`)
#: A private segment for "this room's DL blob", only to hand the DL's start
#: address to the interpreter (GE's room DLs never branch: no G_DL).
SEG_DL = 0x07


class GeInterp(GeDialect, pd_bg.BgInterp):
    """PD's BG interpreter over GE display lists (`ge_gbi`)."""

    def __init__(self, images: Images, texconfigs: dict) -> None:
        super().__init__(types.SimpleNamespace(data=b"", name="bg"), texconfigs, "bg")
        self.images = images


class GeRoom:
    def __init__(self, num: int, pos: tuple[float, float, float], vtx: bytes, pri: bytes | None, sec: bytes | None) -> None:
        self.num = num
        self.pos = pos
        self.vtx = vtx
        self.pri = pri
        self.sec = sec

    @property
    def numvertices(self) -> int:
        return len(self.vtx) // 16

    def images(self) -> set[int]:
        out = set()
        for dl in (self.pri, self.sec):
            for o in range(0, len(dl or b"") - 7, 8):
                w0, w1 = struct.unpack_from(">II", dl, o)
                if w0 >> 24 == pd_fpgun.G_ENDDL:
                    break
                if w0 >> 24 == pd_fpgun.G_SETTEXNUM:
                    out.add(w1 & 0xFFF)
        return out


class GeBg:
    def __init__(self, data: bytes) -> None:
        self.data = data
        _, self.roomtable, self.portaltable, self.viscmds, _ = struct.unpack_from(">5I", data, 0)
        rows = []
        i = 0
        while True:
            pt, pri, sec, x, y, z = struct.unpack_from(">IIIfff", data, self.roomtable - SEG + 24 * i)
            if i > 0 and pri == 0:
                break
            rows.append((pt, pri, sec, (x, y, z)))
            i += 1
        self.rows = rows
        # g_MaxNumRooms = rows with a primary DL, the end marker among them.
        self.maxrooms = len(rows)
        self.rooms = [self._room(r) for r in range(1, self.maxrooms - 1)]

    def _blob(self, ptr: int) -> bytes | None:
        return inflate1172(self.data, ptr - SEG) if ptr else None

    def _room(self, r: int) -> GeRoom:
        pt, pri, sec, pos = self.rows[r]
        return GeRoom(r, pos, self._blob(pt) or b"", self._blob(pri), self._blob(sec))

    def portals(self) -> list[tuple[int, int, int, list[tuple[float, float, float]]]]:
        out = []
        o = self.portaltable - SEG
        while True:
            pv, r1, r2, control = struct.unpack_from(">IBBH", self.data, o)
            if pv == 0:
                return out
            n = self.data[pv - SEG]
            verts = [struct.unpack_from(">3f", self.data, pv - SEG + 4 + 12 * k) for k in range(n)]
            out.append((r1, r2, control, verts))
            o += 8


def fog_rewrite(bg: GeBg) -> int:
    """PD's room rewrite for a fog stage (`bg_load_room`, `bg.c:2971`:
    `gfx_replace_gbi_commands_recursively` with `g_GfxGroup01` on the opaque
    DLs, `g_GfxGroup05` on the translucent ones): under `G_FOG` the RSP writes
    the fog into the shade alpha, so the combiners that take alpha from shade
    (`G_CC_MODULATEIA2`, `MODULATEI2`, ...) are swapped for ones taking it
    from the environment colour (`G_CC_CUSTOM_06`, `_08`, ...). GE's rooms use
    exactly those commands (`FC26A004 1F1093FF` and the rest), and a GE level
    with fog runs in PD's engine as a fog stage, so PD's rule applies: without
    it the translucent layer (railings, glass) takes the fog as its alpha, 0
    near the eye. The groups' render-mode half (`G_RM_PASS` to
    `G_RM_FOG_SHADE_A`) is the materials' `fog_shade` (`ge_extract`).
    Returns how many commands were rewritten."""
    n = 0
    for room in bg.rooms:
        for attr, group in (("pri", 1), ("sec", 5)):
            dl = getattr(room, attr)
            if not dl:
                continue
            pairs = pd_bg.replace_group(group, combines_only=True)
            g = bytearray(dl)
            for off in range(0, len(g) - 7, 8):
                if g[off] == pd_fpgun.G_ENDDL:
                    break
                w = struct.unpack_from(">II", g, off)
                for src, dst in pairs:
                    if w == src:
                        struct.pack_into(">II", g, off, *dst)
                        w = dst
                        n += 1
            setattr(room, attr, bytes(g))
    return n


def interpret(bg: GeBg, images: Images, variant: int = 0):
    """Every room's DLs through the interpreter, PD's order: every room's
    opaque layer, then every room's translucent one (`bg_render_scene`)."""
    cfgs = {n: images.config(n) for room in bg.rooms for n in sorted(room.images())}
    interp = GeInterp(images, cfgs)
    nodes = [{"type": "position", "parent": -1, "pos": [0, 0, 0], "animpart": 0, "mtx": [0, -1, -1], "flags": 0}]
    node_room: dict[int, GeRoom] = {}
    for layer in ("opa", "xlu"):
        for room in bg.rooms:
            dl = room.pri if layer == "opa" else room.sec
            if not dl:
                continue
            ni = len(nodes)
            nodes.append({"type": "dl", "parent": 0, "room": room.num, "layer": layer, "rendermode": 0})
            node_room[ni] = room
            blob = room.vtx + dl
            interp.m = types.SimpleNamespace(data=blob, name=f"room{room.num}")
            interp.d = blob
            pd_bg.bg_default_state(interp.st, variant)
            segs = {SEG_VTX: 0, 0x04: 0, SEG_DL: len(room.vtx)}
            interp.run_node(ni, None, pd_fpgun.MODELRENDERMODE_0, [((SEG_DL << 24), layer == "xlu")], segs, 0)
    return interp, nodes, node_room


def build(ge: Ge, images: Images, level: dict, k: float, code: str, env: dict) -> tuple[dict, dict, list[str]]:
    """The BG as `pd_stage.export_bg` hands `pd_models.write_model` an arena's:
    (model, extra header keys, report lines). `k` takes BG units to PD
    centimetres (the recipe's scale over the level scale)."""
    bg = GeBg(ge.file(level["bg"]))
    rewritten = fog_rewrite(bg) if env["fog"] else 0
    interp, nodes, node_room = interpret(bg, images)
    # The export must not depend on the RDP state a room starts from (the DLs
    # set what they use): re-run from a deliberately different one, as pd_bg.
    alt, _, _ = interpret(bg, images, variant=1)
    diff = sum(1 for a, b in zip(interp.batches, alt.batches)
               if pd_bg.effective(interp.materials[a["material"]]) != pd_bg.effective(alt.materials[b["material"]]))
    if diff or len(alt.batches) != len(interp.batches):
        raise GeError(f"{level['bg']}: the export depends on the assumed starting RDP state ({diff} batches differ)")

    batches = []
    room_bb: dict[int, tuple[list[float], list[float]]] = {}
    for b in interp.batches:
        room = node_room[b["node"]]
        verts = []
        lo, hi = room_bb.setdefault(room.num, ([float("inf")] * 3, [float("-inf")] * 3))
        for v in b["verts"]:
            p = [(room.pos[i] + v["pos"][i]) * k for i in range(3)]
            for i in range(3):
                lo[i] = min(lo[i], p[i])
                hi[i] = max(hi[i], p[i])
            flags = (1 if v["lit"] else 0) | (2 if v["texgen"] else 0)
            verts.append([p[0], p[1], p[2], 0, round(v["uv"][0], 5), round(v["uv"][1], 5), *v["c"], flags])
        # cidx: the vertex's colour, which a GE vertex carries itself: its
        # index in the room's vertex table (the room's colour table is its
        # vertices, `numcolours == numvertices`).
        batches.append({"node": b["node"], "material": b["material"], "verts": verts, "indices": b["indices"],
                        "leaf": 0, "cidx": [v["vsrc"] for v in b["verts"]]})

    portals = []
    for r1, r2, _control, verts in bg.portals():
        pv = [[c * k for c in v] for v in verts]
        portals.append({"rooms": [r1, r2], "flags": 0, "verts": pv})
        # g_Rooms[].bbmin/bbmax take in the room's portals (as a PD room's
        # section 3 box does), so a portal never pokes out of either room.
        for r in (r1, r2):
            if r in room_bb:
                lo, hi = room_bb[r]
                for v in pv:
                    for i in range(3):
                        lo[i] = min(lo[i], v[i])
                        hi[i] = max(hi[i], v[i])

    room_nodes = {(n["room"], n["layer"]): i for i, n in enumerate(nodes) if n["type"] == "dl"}
    gfx_bb: dict[int, tuple[list[float], list[float]]] = {}
    for b in batches:
        r = nodes[b["node"]]["room"]
        lo, hi = gfx_bb.setdefault(r, ([float("inf")] * 3, [float("-inf")] * 3))
        for v in b["verts"]:
            for i in range(3):
                lo[i] = min(lo[i], v[i])
                hi[i] = max(hi[i], v[i])
    rooms = []
    for room in bg.rooms:
        bb = room_bb.get(room.num)
        gb = gfx_bb.get(room.num, (None, None))
        rooms.append({
            "room": room.num, "pos": [c * k for c in room.pos],
            "bbmin": bb[0] if bb else None, "bbmax": bb[1] if bb else None, "gfx_bbmin": gb[0], "gfx_bbmax": gb[1],
            # GE rooms carry no brightness range or lights: the arenas' defaults.
            "br_light_min": 128, "br_light_max": 255, "numlights": 0, "lightindex": -1,
            "opa_node": room_nodes.get((room.num, "opa")), "xlu_node": room_nodes.get((room.num, "xlu")),
            "numvertices": room.numvertices, "numcolours": room.numvertices, "colour_alpha_only": [], "bsp_parents": 0,
        })

    used = sorted({m["texture"]["id"] & 0xFFFF for m in interp.materials if m["texture"]})
    textures = {str(0x10000 | n): images.entry(n) for n in used}
    stage = f"STAGE_CUSTOM_{code.upper()}"
    model = {"name": f"bg_{code}", "source": f"ge007.u {level['bg']} (ROM)", "nummatrices": 1, "nodes": nodes, "parts": {},
             "materials": interp.materials, "batches": batches, "skel": None, "textures": textures}
    extra = {"stage": stage, "units": f"cm (world; GE BG units x {k:.6g})", "env": env, "rooms": rooms, "portals": portals,
             # SUBST: GE's global visibility commands (`bg.c:3976`; PD's BGCMD
             # set) are dropped / the portal pass alone decides what is drawn,
             # as on every Combat Simulator arena (their g_BgCommands are empty).
             "bgcmds": [[0, 1, 0]], "lights": [], "section2_textures": []}
    ntri = sum(len(b["indices"]) // 3 for b in batches)
    xlu = sum(len(b["indices"]) // 3 for b in batches if nodes[b["node"]]["layer"] == "xlu")
    report = [f"bg: {len(bg.rooms)} rooms, {len(portals)} portals, {ntri} triangles ({xlu} translucent), "
              f"{len(interp.materials)} materials, {len(textures)} textures; {rewritten} commands rewritten for fog (PD's g_GfxGroup01/05)"]
    report += [f"  WARN {w}" for w in sorted(set(interp.warnings))]
    return model, extra, report


def grow_rooms_to_portals(extra: dict) -> None:
    """Grow each room's box (`bbmin`/`bbmax`) to take in its portals again,
    after a portal moved (`ge_setup.align_portals`)."""
    rooms = {r["room"]: r for r in extra["rooms"] if r["bbmin"]}
    for portal in extra["portals"]:
        for num in portal["rooms"]:
            r = rooms.get(num)
            if r is None:
                continue
            for v in portal["verts"]:
                for i in range(3):
                    r["bbmin"][i] = min(r["bbmin"][i], v[i])
                    r["bbmax"][i] = max(r["bbmax"][i], v[i])


def opaque_triangles(model: dict) -> list[tuple[tuple[float, float, float], ...]]:
    """The BG's opaque-layer triangles, world cm (for `ge_stan.Solid`)."""
    out = []
    xlu = {i for i, n in enumerate(model["nodes"]) if n.get("layer") == "xlu"}
    for b in model["batches"]:
        if b["node"] in xlu:
            continue
        v = b["verts"]
        idx = b["indices"]
        for j in range(0, len(idx) - 2, 3):
            out.append(tuple(tuple(v[idx[j + q]][:3]) for q in range(3)))
    return out
