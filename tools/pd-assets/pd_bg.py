#!/usr/bin/env python3
"""Export Perfect Dark's textured level geometry ("BG") for one stage, faithfully.

The BG file (`files/bgdata/bg_<stage>.seg`) is decoded exactly as the game loads
it, and every room's display lists are replayed through `pd_fpgun.Interp` — the
same GBI interpreter the first-person guns use — so the Rust side gets the
per-batch N64 draw state (combiner, blender, z-mode, texture tile) rather than a
guess. `build` returns a model dict (one POSITION root, one DL node per room per
layer) plus a `rooms` table; `pd_stage.py` writes it as `stages/<code>/bg.json`
+ `.bg.bin` in the one model format, with its textures in the pool.

What is ported, with its source (decomp = reference/pd-decomp/src):

* **File layout** — `bg_reset` (game/bg.c:1470): header `u32 inflatedsize,
  section1compsize, primcompsize`; the rzip'd primary data at 0xc
  (bg.c:1538); `var8007fc54 = inflatedsize - primcompsize - 0xc`
  (bg.c:1519-1520); primary `u32[1]` = `g_BgRooms` (bg.c:1608), pointers are
  `0x0f000000`-based; `roomcount` = entries from 1 until `unk00 == 0`
  (bg.c:1612). rzip = `0x11 0x73`, 3-byte length, raw DEFLATE
  (`pd_tex.rzip_inflate`, lib/rzip.s).
* **Rooms** — `bg_load_room` (bg.c:2738): room r's data is at file offset
  `g_BgRooms[r].unk00 - 0x0f000000 - var8007fc54` (bg.c:2799-2800), length
  `g_BgRooms[r+1].unk00 - g_BgRooms[r].unk00`, rzip'd; rooms 1..roomcount-1
  only (bg.c:2767). Inflated = `struct roomgfxdata` (include/types.h:3546);
  `struct roomblock` (types.h:3530) is 20 bytes; every offset is promoted by
  subtracting `unk00` (bg.c:2815-2876). The blocks array runs up to
  `gfxdata->vertices`, shortened by any parent's `vertices` slot (bg.c:2842-2876).
* **Draw order** — `bg_render_room_pass` (bg.c:3147): a leaf binds segment 0x0e
  (SPSEGMENT_BG_VTX, constants.h:3924) to `block->vertices` and 0x0d
  (SPSEGMENT_BG_COL, :3923) to `block->colours`, runs `block->gdl`, then its
  `next`; a parent draws `child` and `child->next` in an order chosen by the
  camera's side of the plane `unk0c[0]` (point) / `unk0c[1]` (normal)
  (bg.c:3182-3216). Opaque rooms are all drawn before translucent ones
  (`bg_render_scene`, bg.c:1133 vs :1204).
* **Texture expansion** — `tex_load_from_gdl` (game/tex.c:823): the C0 command
  becomes tile setup, modelled by `pd_fpgun.Interp.c0`. Its vertex `s,t >>= 1`
  rewrite (tex.c:1036-1047) is gated on `tex->unk0c_03` (tex.c:894), which is only ever
  written `false` (texdecompress.c:2257), so it is never applied.
* **Environment rewrite** — `bg_load_room` (bg.c:2971-2977) runs
  `gfx_replace_gbi_commands_recursively` (game/gfxreplace.c:326) with group 1/5
  when fog is on, else groups 6/7 when the stage has no transparency. The flags
  come from `env_choose_and_apply` (game/env.c:302): a stage in
  `g_FogEnvironments` (env.c:50, loop :328) enables fog, otherwise its
  `g_NoFogEnvironments` row (env.c:69, loop :339; Complex = :105) sets `g_FogEnabled = false` and
  `g_EnvHasTransparency = row.transparency` (env.c:281-283). Both tables and
  the replacement groups are PARSED from the decomp, not transcribed.
* **Positions** — world = `g_BgRooms[r].pos + vtx` (room.c:125 room matrix,
  bg.c:4170 hit test). Stage scale for Complex is 1 (stagetable.c:22), so world
  units are centimetres, the same space as `tiles/ref.json`.
* **Colours** — exported RAW (the baked vertex lighting). At render,
  `room_highlight` (game/dlights.c:1626) rescales them by the room brightness;
  its alpha-only path is keyed on `gfxdata->vertices[i].flags & 1` with `i` a
  COLOUR index (a PD bug, dlights.c:1668) — `rooms[].colour_alpha_only` lists
  those colour indices and every batch carries `cidx` (per vertex, the index
  into the room's colour table) so that path can be replayed.

Usage:
    python tools/pd-assets/pd_bg.py ref     # interpret and print a summary (pd_stage.py writes it)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import struct
import sys
import types

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pd_fpgun  # noqa: E402
import pd_gltf  # noqa: E402
import pd_tex  # noqa: E402
from pd_fpgun import (  # noqa: E402
    G_COL, G_DL, G_ENDDL, G_SETCOMBINE, G_SETTEXNUM, G_VTX, VTX_SIZE,
    GM_CULL_BACK, GM_SHADE, GM_SHADING_SMOOTH, GM_ZBUFFER,
    G_CYC_2CYCLE, MDSFT_TEXTFILT, MDSFT_TEXTLOD, RM_AA_ZB_OPA_SURF2, CC_TRILERP_MODULATEIA2,
)

from pd_paths import ASSETS, DECOMP, REPO, SRC, decomp_rel  # noqa: E402,F401

#: stage file stem -> (STAGE_* constant, bg file). stagetable.c:22 for ref.
STAGES = {
    "ref": ("STAGE_MP_COMPLEX", "bg_ref.seg"),
}

SEG_BG_COL = 0x0D  # SPSEGMENT_BG_COL (constants.h:3923)
SEG_BG_VTX = 0x0E  # SPSEGMENT_BG_VTX (constants.h:3924)
#: A private segment for "this room's inflated data at offset X", used only to
#: hand DL start addresses to the interpreter. BG DLs never branch (no G_DL is
#: present — checked below), so it is never dereferenced by the DL itself.
SEG_ROOMDATA = 0x0F

ROOMBLOCKTYPE_LEAF, ROOMBLOCKTYPE_PARENT = 0, 1  # constants.h:3609
ROOMBLOCK_SIZE = 20  # u8 type (+3 pad), next, 3 x u32 union — types.h:3530
ROOMGFXDATA_HDR = 0x18  # types.h:3546 (`blocks` at 0x18)


# ---------------------------------------------------------------------------
# Small C-source readers (so the rules below are the decomp's, not a copy)
# ---------------------------------------------------------------------------


def _read(*parts: str) -> str:
    with open(os.path.join(SRC, *parts), encoding="utf-8", errors="replace") as fh:
        return fh.read()


def _strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def stage_constant(name: str) -> int:
    m = re.search(rf"#define\s+{name}\s+(0x[0-9a-fA-F]+|\d+)", _read("include", "constants.h"))
    if not m:
        raise SystemExit(f"{name} not in constants.h")
    return int(m.group(1), 0)


def _struct_fields(name: str) -> list[str]:
    """The field names of `struct <name>` in include/types.h, in order."""
    text = _strip_comments(_read("include", "types.h"))
    body = re.search(rf"struct\s+{name}\s*\{{(.*?)\}};", text, re.S).group(1)
    return [m.group(1) for m in re.finditer(r"(\w+)\s*(?:\[[^\]]*\])?\s*;", body)]


def _env_row(row: str, fields: list[str]) -> dict:
    """One `g_NoFogEnvironments` / `g_FogEnvironments` initialiser as a dict by
    field name, with env.c's `RGB(col)` (three bytes), `NO_SUNS` (0, NULL) and
    `SUNS(arr)` macros expanded (env.c:31-33)."""
    vals: list = []
    for tok in [t.strip() for t in re.split(r",(?![^(]*\))", row) if t.strip()]:
        m = re.fullmatch(r"RGB\((0x[0-9a-fA-F]+)\)", tok)
        if m:
            c = int(m.group(1), 16)
            vals += [c >> 16, (c >> 8) & 0xFF, c & 0xFF]
        elif tok == "NO_SUNS":
            vals += [0, None]
        elif tok.startswith("SUNS("):
            vals += [None, tok[5:-1]]
        else:
            try:
                vals.append(float(tok.rstrip("f")) if "." in tok else int(tok, 0))
            except ValueError:
                vals.append(tok)
    return dict(zip(fields, vals))


def env_flags(stage_name: str) -> tuple[bool, bool, str, dict]:
    """(g_FogEnabled, g_EnvHasTransparency, provenance, the row) for a stage, per
    `env_choose_and_apply` (env.c:302): the fog table wins (env.c:328-335);
    otherwise the LAST matching no-fog row (env.c:339-343), else row 0 (:348).
    The row is keyed by `struct nofogenvironment`'s fields (types.h:3224)."""
    text = _strip_comments(_read("game", "env.c"))
    fog = re.search(r"g_FogEnvironments\[\]\s*=\s*\{(.*?)\n\};", text, re.S).group(1)
    nofog = re.search(r"g_NoFogEnvironments\[\]\s*=\s*\{(.*?)\n\};", text, re.S).group(1)
    for row in re.findall(r"\{([^{}]*)\}", fog):
        if row.split(",")[0].strip() == stage_name:
            return True, False, "g_FogEnvironments row", {}
    rows = re.findall(r"\{([^{}]*)\}", nofog)
    chosen = None
    for row in rows:
        if row.split(",")[0].strip() == stage_name:
            chosen = row
    if chosen is None:
        chosen = rows[0]
    env = _env_row(chosen, _struct_fields("nofogenvironment"))
    transparency = env["transparency"]
    return False, bool(transparency), f"g_NoFogEnvironments row, transparency={transparency}", env


_CCMUX = {"COMBINED": 0, "TEXEL0": 1, "TEXEL1": 2, "PRIMITIVE": 3, "SHADE": 4, "ENVIRONMENT": 5,
          "CENTER": 6, "SCALE": 6, "COMBINED_ALPHA": 7, "TEXEL0_ALPHA": 8, "TEXEL1_ALPHA": 9,
          "PRIMITIVE_ALPHA": 10, "SHADE_ALPHA": 11, "ENV_ALPHA": 12, "LOD_FRACTION": 13,
          "PRIM_LOD_FRAC": 14, "NOISE": 7, "K4": 7, "K5": 15, "1": 6, "0": 31}  # gbi.h:364-384
_ACMUX = {"COMBINED": 0, "TEXEL0": 1, "TEXEL1": 2, "PRIMITIVE": 3, "SHADE": 4, "ENVIRONMENT": 5,
          "LOD_FRACTION": 0, "PRIM_LOD_FRAC": 6, "1": 6, "0": 7}  # gbi.h:387-396


def cc_macros() -> dict[str, list[str]]:
    out = {}
    for f in (("include", "PR", "gbi.h"), ("include", "gbiex.h")):
        path = os.path.join(SRC, *f)
        if not os.path.exists(path):
            path = os.path.join(DECOMP, *f)
        with open(path, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                m = re.match(r"\s*#define\s+(G_CC_\w+)\s+(.+)", line)
                if m:
                    toks = [t.strip() for t in _strip_comments(m.group(2)).split(",")]
                    if len(toks) == 8:
                        out[m.group(1)] = toks
    return out


def combine_words(mode1: list[str], mode2: list[str]) -> tuple[int, int]:
    """`gsDPSetCombineMode(a, b)` -> (w0, w1) via gsDPSetCombineLERP (gbi.h:2315),
    _SHIFTL masking each mux to its field width."""
    a0, b0, c0, d0, Aa0, Ab0, Ac0, Ad0 = mode1
    a1, b1, c1, d1, Aa1, Ab1, Ac1, Ad1 = mode2
    C = lambda n, w: _CCMUX[n] & ((1 << w) - 1)  # noqa: E731
    A = lambda n: _ACMUX[n] & 7  # noqa: E731
    w0 = (G_SETCOMBINE << 24) | (C(a0, 4) << 20) | (C(c0, 5) << 15) | (A(Aa0) << 12) | (A(Ac0) << 9) \
        | (C(a1, 4) << 5) | C(c1, 5)
    w1 = (C(b0, 4) << 28) | (C(d0, 3) << 15) | (A(Ab0) << 12) | (A(Ad0) << 9) \
        | (C(b1, 4) << 24) | (A(Aa1) << 21) | (A(Ac1) << 18) | (C(d1, 3) << 6) | (A(Ab1) << 3) | A(Ad1)
    return w0 & 0xFFFFFFFF, w1 & 0xFFFFFFFF


def replace_group(n: int) -> list[tuple[tuple[int, int], tuple[int, int]]]:
    """`g_GfxGroupNN` (gfxreplace.c) as find/replace word pairs, in order."""
    text = _strip_comments(_read("game", "gfxreplace.c"))
    body = re.search(rf"Gfx\s+g_GfxGroup{n:02d}\[\]\s*=\s*\{{(.*?)\n\}};", text, re.S).group(1)
    macros = cc_macros()
    cmds = []
    for m in re.finditer(r"gs(DPSetCombineMode|DPSetRenderMode|DPSetCycleType)\s*\(([^()]*)\)", body):
        if m.group(1) != "DPSetCombineMode":
            raise NotImplementedError(f"g_GfxGroup{n:02d} uses {m.group(1)} (fog stages are not ported)")
        x, y = [t.strip() for t in m.group(2).split(",")]
        cmds.append(combine_words(macros[x], macros[y]))
    if len(cmds) % 2:
        raise SystemExit(f"g_GfxGroup{n:02d}: odd command count")
    return [(cmds[i], cmds[i + 1]) for i in range(0, len(cmds), 2)]


def tiles_bbox(stem: str):
    path = os.path.join(ASSETS, "tiles", f"{stem}.json")
    with open(path, encoding="utf-8") as fh:
        rooms = json.load(fh)["rooms"]
    lo = [float("inf")] * 3
    hi = [float("-inf")] * 3
    nonempty = 0
    for tiles in rooms.values():
        if tiles:
            nonempty += 1
        for t in tiles:
            for v in t["vertices"]:
                for i, k in enumerate("xyz"):
                    lo[i] = min(lo[i], v[k])
                    hi[i] = max(hi[i], v[k])
    return lo, hi, len(rooms), nonempty


# ---------------------------------------------------------------------------
# The BG file
# ---------------------------------------------------------------------------


class Room:
    def __init__(self, num: int, unk00: int, pos, lmin: int, lmax: int):
        self.num = num
        self.unk00 = unk00
        self.pos = pos
        self.br_light_min = lmin
        self.br_light_max = lmax
        self.data: bytearray | None = None
        self.vertices = self.colours = self.opablocks = self.xlublocks = None
        self.numvertices = self.numcolours = 0
        self.blocks: dict[int, tuple] = {}  # offset -> (type, next, a, b, c)
        self.opa_leaves: list[int] = []
        self.xlu_leaves: list[int] = []
        self.parents = 0


class BgFile:
    def __init__(self, path: str):
        with open(path, "rb") as fh:
            self.file = fh.read()
        d = self.file
        inflatedsize, self.section1compsize, primcompsize = struct.unpack_from(">III", d, 0)
        self.fc54 = inflatedsize - primcompsize - 0xC  # bg.c:1519-1520
        self.primary, _ = pd_tex.rzip_inflate(d, 0xC)
        if len(self.primary) != inflatedsize:
            raise SystemExit(f"primary inflated to {len(self.primary)}, header says {inflatedsize}")
        p = self.primary
        hdr = struct.unpack_from(">6I", p, 0)
        if hdr[0] != 0:
            raise SystemExit("primary u32[0] != 0 (bg.c:1606 path not taken)")
        roff = hdr[1] - 0x0F000000
        self.bgrooms = []
        j = 0
        while True:
            unk00, x, y, z, lmin, lmax = struct.unpack_from(">IfffBB", p, roff + j * 20)  # types.h:5075
            self.bgrooms.append((unk00, (x, y, z), lmin, lmax))
            if j > 0 and unk00 == 0:
                break
            j += 1
        self.roomcount = 0
        j = 1
        while self.bgrooms[j][0] != 0:  # bg.c:1612
            self.roomcount += 1
            j += 1
        # Section 2 (bg.c:1544-1580): the stage's texture id list.
        s2 = self.section1compsize + 0xC
        h0, s2comp = struct.unpack_from(">HH", d, s2)
        raw = d[s2 + 4 : s2 + 4 + s2comp]
        if raw[:2] == b"\x11\x73":
            sec2, _ = pd_tex.rzip_inflate(raw, 0)
        else:
            sec2 = raw
        n = (h0 & 0x7FFF) >> 1
        self.section2_textures = [struct.unpack_from(">H", sec2, 2 * i)[0] for i in range(n)]

    def load_room(self, r: int) -> Room:
        """`bg_load_room` (bg.c:2738) minus the memory juggling."""
        unk00, pos, lmin, lmax = self.bgrooms[r]
        room = Room(r, unk00, pos, lmin, lmax)
        nxt = self.bgrooms[r + 1][0]
        fileoff = unk00 - 0x0F000000 - self.fc54
        raw = self.file[fileoff : fileoff + (nxt - unk00)]
        if raw[:2] == b"\x11\x73":
            data, _ = pd_tex.rzip_inflate(raw, 0)
        else:
            data = raw  # bg_inflate's bcopy path (bg.c:2624)
        room.data = bytearray(data)
        g = room.data

        def rel(ptr: int) -> int | None:
            return (ptr - unk00) if ptr else None

        vtx, col, opa, xlu = struct.unpack_from(">IIII", g, 0)
        room.vertices, room.colours, room.opablocks, room.xlublocks = rel(vtx), rel(col), rel(opa), rel(xlu)
        end = room.vertices
        b = ROOMGFXDATA_HDR
        while b + ROOMBLOCK_SIZE <= end:  # bg.c:2842
            t, nx, a, bb, c = struct.unpack_from(">BxxxIIII", g, b)
            if t == ROOMBLOCKTYPE_LEAF:
                room.blocks[b] = (t, rel(nx), rel(a), rel(bb), rel(c))
            elif t == ROOMBLOCKTYPE_PARENT:
                room.blocks[b] = (t, rel(nx), rel(a), rel(bb), None)
                room.parents += 1
                if rel(bb) is not None and rel(bb) < end:
                    end = rel(bb)
            b += ROOMBLOCK_SIZE
        room.numvertices = (room.colours - room.vertices) // VTX_SIZE  # bg.c:2879
        first_gdl = min(bl[2] for bl in room.blocks.values() if bl[0] == ROOMBLOCKTYPE_LEAF and bl[2])
        room.numcolours = (first_gdl - room.colours) // 4  # bg.c:2880
        room.opa_leaves = self.leaves(room, room.opablocks, True)
        room.xlu_leaves = self.leaves(room, room.xlublocks, True)
        return room

    def leaves(self, room: Room, block: int | None, follow_next: bool) -> list[int]:
        """Leaf blocks in `bg_render_room_pass` order (bg.c:3147). For a parent,
        the camera picks the order of its two children; we take the `sum < 0`
        branch (`child`, then `child->next`, bg.c:3205) — irrelevant for the
        opaque layer (z-buffered), and recorded as `bsp` on the node for xlu."""
        out = []
        while block is not None:
            t, nx, a, bb, _c = room.blocks[block]
            if t == ROOMBLOCKTYPE_LEAF:
                out.append(block)
            elif t == ROOMBLOCKTYPE_PARENT:
                if a is None:
                    break  # a childless parent also ends the chain (bg.c:3183, :3214)
                child = a
                child_next = room.blocks[child][1]
                out += self.leaves(room, child, False)
                out += self.leaves(room, child_next, False)
            else:
                break  # `default` type: nothing drawn, chain not followed
            if not follow_next:
                break
            block = nx
        return out


def gfx_replace(room: Room, leaves: list[int], group) -> int:
    """`gfx_replace_gbi_commands` (gfxreplace.c:293) over each leaf's DL, in
    place. Every pair is tried in order against the CURRENT command, exactly as
    the C loop does."""
    g = room.data
    n = 0
    for leaf in leaves:
        off = room.blocks[leaf][2]
        while off + 8 <= len(g) and g[off] != G_ENDDL:
            w = struct.unpack_from(">II", g, off)
            for src, dst in group:
                if w == src:
                    struct.pack_into(">II", g, off, *dst)
                    w = dst
                    n += 1
            off += 8
    return n


# ---------------------------------------------------------------------------
# Interpretation
# ---------------------------------------------------------------------------


class BgInterp(pd_fpgun.Interp):
    """`pd_fpgun.Interp` with the one gun-specific behaviour switched off: BG
    display lists set their own cull state before drawing (measured: the
    `variant=1` re-run in `export` starts with cull off and must match), so the cull a batch was drawn with is recorded
    concretely instead of as "inherit"."""

    def material_key(self, lit: bool, texgen: bool) -> int:
        saved = self.st.cull_touched
        self.st.cull_touched = True
        try:
            return super().material_key(lit, texgen)
        finally:
            self.st.cull_touched = saved


def bg_default_state(st: pd_fpgun.RspState, variant: int = 0) -> None:
    """The RSP/RDP state a room's DL starts from. Nothing in `bg_render` /
    `bg_render_room_opaque` sets combiner, render mode or env before
    `bg_render_room_pass` (bg.c:2173, :3226) — only matrices, lights and
    segments — so it is whatever the previous draw left. The DLs set everything
    they use themselves; `variant=1` is a deliberately different starting state
    used to PROVE that (the export must not change)."""
    if variant == 0:
        st.geom = GM_ZBUFFER | GM_SHADE | GM_SHADING_SMOOTH | GM_CULL_BACK
        st.omh = G_CYC_2CYCLE | (1 << MDSFT_TEXTLOD) | (2 << MDSFT_TEXTFILT)
        st.oml = RM_AA_ZB_OPA_SURF2
        st.combine = CC_TRILERP_MODULATEIA2
        st.prim = [255, 255, 255, 255]
        st.env = None
        st.fog = None
    else:
        st.geom = GM_ZBUFFER | GM_SHADE | GM_SHADING_SMOOTH  # previous room/prop left cull off
        st.omh = 0
        st.oml = pd_fpgun.OML_BONDGUN_XLU
        st.combine = pd_fpgun.CC_MODULATEIA
        st.prim = [1, 2, 3, 4]
        st.env = [9, 9, 9, 9]
        st.fog = [7, 7, 7, 7]
    st.tex_s = st.tex_t = 0xFFFF
    st.tex_on = True
    st.tile = pd_fpgun.Tile()
    st.colours_off = None
    st.cur_mtx = 0


def vertex_info(room: Room, leaf: int, dl_off: int) -> list[tuple[int, int, int]]:
    """Per vertex LOADED by this leaf's DL (in `Interp`'s uid order):
    `(cidx, s, t)` — the index into the room's whole colour table
    (`bg_render_room_pass`'s `((block->colours - addr) >> 2)`, bg.c:3169, plus
    the G_COL offset plus the vertex's colour byte / 4) and the raw texture
    coordinates (for dyntex). Mirrors `Interp.run`'s G_VTX loop exactly."""
    g = room.data
    _t, _nx, _gdl, vbase, cbase = room.blocks[leaf]
    out = []
    col_off = None
    off = dl_off
    while off + 8 <= len(g):
        w0, w1 = struct.unpack_from(">II", g, off)
        off += 8
        op = w0 >> 24
        if op == G_ENDDL:
            break
        if op == G_COL:
            col_off = cbase + (w1 & 0xFFFFFF) if (w1 >> 24) == SEG_BG_COL else None
        elif op == G_VTX:
            n = (w0 & 0xFFFF) // VTX_SIZE
            dest = (w0 >> 16) & 0xF
            src = vbase + (w1 & 0xFFFFFF)
            for i in range(n):
                vo = src + i * VTX_SIZE
                if vo + VTX_SIZE > len(g) or dest + i >= 16:
                    break
                ci = g[vo + 7] >> 2
                co = (col_off if col_off is not None else cbase) + ci * 4
                s, t = struct.unpack_from(">hh", g, vo + 8)
                out.append(((co - room.colours) // 4, s, t))
    return out


#: Textures `tex_load_from_gdl` flags for per-frame UV animation when the room
#: is a dyntex room (tex.c:955-1008; `dyntex_has_room` is true while a room
#: loads, bg.c:2794 / dyntex.c:426).
DYNTEX_TEXTURES = {0x06CB: "river", 0x0A6A: "powerjuice", 0x0A69: "powerring", 0x06E2: "teleportal",
                   0x01C7: "river", 0x0DAE: "river", 0x029B: "monitor", 0x090F: "ocean", 0x0A42: "arrows"}


def walk_blocks(room: Room, block: int | None):
    """Every block reachable from `block` (next chains and children)."""
    stack = [block]
    while stack:
        b = stack.pop()
        if b is None:
            continue
        yield b
        t, nx, a, _bb, _c = room.blocks[b]
        stack.append(nx)
        if t == ROOMBLOCKTYPE_PARENT:
            stack.append(a)


def bsp_tree(room: Room, block: int | None, leaves: list[int]) -> list:
    """The layer's block tree for camera-dependent ordering, as
    `bg_render_room_pass` walks it (bg.c:3147): a list (the `next` chain) of
    `{"leaf": ordinal}` or `{"plane": [px,py,pz, nx,ny,nz], "a": item, "b": item}`
    where `a` = `child`, `b` = `child->next`, and the rule is
    `sum = n . (p - cam_pos); sum < 0 ? draw a then b : draw b then a`
    (bg.c:3190-3212; world coordinates). Batches are exported in the `a`-first
    order and carry their `leaf` ordinal."""

    def item(b: int):
        t, _nx, a, bb, _c = room.blocks[b]
        if t == ROOMBLOCKTYPE_LEAF:
            return {"leaf": leaves.index(b)}
        plane = list(struct.unpack_from(">6f", room.data, bb))
        child_next = room.blocks[a][1]
        return {"plane": [round(x, 4) for x in plane], "a": item(a),
                "b": item(child_next) if child_next is not None else None}

    out = []
    while block is not None:
        t, nx, a, _bb, _c = room.blocks[block]
        if t == ROOMBLOCKTYPE_PARENT and a is None:
            break  # same stop rule as `BgFile.leaves`
        out.append(item(block))
        block = nx
    return out


_TEXDIMS: dict[int, tuple[int, int, int]] = {}


def texture_configs(rooms: list[Room]) -> dict[int, pd_gltf.TexConfig]:
    """A `read_texconfigs`-shaped table for the global texture numbers the C0
    commands name (`tex_load_from_gdl`, tex.c:886: `texturenum = w1 & 0xfff`),
    so `Interp.material_key` resolves them like a model's pool textures."""
    out = {}
    for room in rooms:
        for leaf in room.opa_leaves + room.xlu_leaves:
            off = room.blocks[leaf][2]
            while off + 8 <= len(room.data) and room.data[off] != G_ENDDL:
                w0, w1 = struct.unpack_from(">II", room.data, off)
                if w0 >> 24 == G_SETTEXNUM:
                    texnum = w1 & 0xFFF
                    if texnum not in out:
                        if texnum not in _TEXDIMS:
                            try:
                                t = pd_tex.load(texnum)
                                _TEXDIMS[texnum] = (t.width, t.height, t.numlods)
                            except (pd_tex.UnsupportedTexture, OSError):
                                _TEXDIMS[texnum] = (0, 0, 0)
                        w, h, lods = _TEXDIMS[texnum]
                        out[texnum] = pd_gltf.TexConfig(texnum, texnum, w, h, lods, 0, 0, texnum=texnum)
                off += 8
    return out


def interpret(bg: BgFile, rooms: list[Room], variant: int = 0):
    interp = BgInterp(types.SimpleNamespace(data=b"", name="bg"), texture_configs(rooms), "bg")
    nodes = [{"type": "position", "parent": -1, "pos": [0, 0, 0], "animpart": 0, "mtx": [0, -1, -1], "flags": 0}]
    node_room: dict[int, Room] = {}
    batch_extra: list[dict] = []
    stats = {"gdl_branches": 0, "lit_verts": 0, "texgen_verts": 0}
    for layer in ("opa", "xlu"):
        for room in rooms:
            leaves = room.opa_leaves if layer == "opa" else room.xlu_leaves
            if not leaves:
                continue
            ni = len(nodes)
            node = {"type": "dl", "parent": 0, "room": room.num, "layer": layer, "rendermode": 0}
            root = room.opablocks if layer == "opa" else room.xlublocks
            if any(room.blocks[b][0] == ROOMBLOCKTYPE_PARENT for b in walk_blocks(room, root)):
                node["tree"] = bsp_tree(room, root, leaves)
            nodes.append(node)
            node_room[ni] = room
            interp.m = types.SimpleNamespace(data=bytes(room.data), name=f"room{room.num}")
            interp.d = interp.m.data
            bg_default_state(interp.st, variant)
            for leafno, leaf in enumerate(leaves):
                _t, _nx, gdl, vbase, cbase = room.blocks[leaf]
                # Branches would need real segment 0x0f (the primary data); none exist.
                o = gdl
                while o + 8 <= len(room.data) and room.data[o] != G_ENDDL:
                    if room.data[o] == G_DL:
                        stats["gdl_branches"] += 1
                    o += 8
                segs = {SEG_BG_VTX: vbase, SEG_BG_COL: cbase, 6: cbase, SEG_ROOMDATA: 0}
                first = len(interp.batches)
                interp.run_node(ni, None, pd_fpgun.MODELRENDERMODE_0, [((SEG_ROOMDATA << 24) | gdl, False)],
                                segs, 0)
                info = vertex_info(room, leaf, gdl)
                for b in interp.batches[first:]:
                    extra = {"leaf": leafno, "cidx": [info[v["uid"][1] - 1][0] for v in b["verts"]]}
                    tex = interp.materials[b["material"]]["texture"]
                    if tex and tex["id"] in DYNTEX_TEXTURES:
                        # tex_load_from_gdl marks these vertices animated (tex.c:955-1008);
                        # dyntex_tick_room rewrites their s,t every frame (dyntex.c:150).
                        extra["dyntex"] = DYNTEX_TEXTURES[tex["id"]]
                        extra["st"] = [list(info[v["uid"][1] - 1][1:]) for v in b["verts"]]
                    batch_extra.append(extra)
                    for v in b["verts"]:
                        stats["lit_verts"] += bool(v["lit"])
                        stats["texgen_verts"] += bool(v["texgen"])
    return interp, nodes, node_room, batch_extra, stats


def reads_prim(mat: dict) -> bool:
    """Does the combiner read PRIMITIVE / PRIM_LOD_FRAC? (fast3d mux order;
    colour a/b/d: 3 = PRIM; c: 3 PRIM, 10 PRIM_A, 14 PRIM_LOD_FRAC; alpha
    a/b/d: 3 PRIM; alpha c: 3 PRIM, 6 PRIM_LOD_FRAC — gbi.h:364-396)."""
    for cyc in (0, 1):  # both, conservatively (1-cycle normally duplicates them)
        a, b, c, d, Aa, Ab, Ac, Ad = mat["combine"][8 * cyc : 8 * cyc + 8]
        if 3 in (a, b, d) or c in (3, 10, 14) or 3 in (Aa, Ab, Ad) or Ac in (3, 6):
            return True
    return False


def effective(mat: dict) -> dict:
    m = dict(mat)
    if not reads_prim(m):
        m["prim"] = None
    if not m["fog_tint"]:
        m["fog"] = None
    return m


def flatten(interp, node_room, batch_extra):
    batches = []
    for b, extra in zip(interp.batches, batch_extra):
        room = node_room[b["node"]]
        px, py, pz = room.pos
        verts = []
        for v in b["verts"]:
            flags = (1 if v["lit"] else 0) | (2 if v["texgen"] else 0)
            verts.append([px + v["pos"][0], py + v["pos"][1], pz + v["pos"][2], 0,
                          round(v["uv"][0], 5), round(v["uv"][1], 5),
                          v["c"][0], v["c"][1], v["c"][2], v["c"][3], flags])
        batches.append({"node": b["node"], "material": b["material"], "verts": verts,
                        "indices": b["indices"], **extra})
    return batches


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------


def build(stem: str) -> dict:
    """Interpret stage `stem`'s BG exactly as `bg_reset` + `bg_load_room` load it.

    Returns the model dict (`nodes`, `materials`, `batches` with world-space
    vertices, `rooms`, `env`, `section2_textures`, provenance) plus `used`, the
    global texture numbers its materials sample, and `warnings`. Nothing is
    written: `pd_stage.py` writes it in the one model format, its textures in
    the pool."""
    stage_name, fname = STAGES[stem]
    path = os.path.join(ASSETS, "files", "bgdata", fname)
    bg = BgFile(path)
    warnings: list[str] = []
    rooms = [bg.load_room(r) for r in range(1, bg.roomcount)]  # bg.c:2767

    fog, transparency, env_src, env_row = env_flags(stage_name)
    replaced = 0
    if fog:
        raise NotImplementedError(f"{stage_name} runs with fog (groups 1/5 not ported)")
    if not transparency:  # bg.c:2974
        g6, g7 = replace_group(6), replace_group(7)
        for room in rooms:
            replaced += gfx_replace(room, room.opa_leaves, g6)
            replaced += gfx_replace(room, room.xlu_leaves, g7)

    interp, nodes, node_room, batch_extra, stats = interpret(bg, rooms)
    if stats["gdl_branches"]:
        warnings.append(f"{stats['gdl_branches']} G_DL branches in room DLs were not followed")
    if stats["lit_verts"] or stats["texgen_verts"]:
        warnings.append(f"{stats['lit_verts']} lit / {stats['texgen_verts']} texgen BG vertices")
    warnings += interp.warnings

    # Starting-state independence: re-run from a deliberately different state.
    # Only the state a batch can actually SEE is compared: prim when the
    # combiner reads PRIMITIVE / PRIM_LOD_FRAC, fog colour when the blender
    # reads it (neither is true anywhere in Complex).
    alt, *_ = interpret(bg, rooms, variant=1)
    diff = 0
    if len(alt.batches) != len(interp.batches):
        diff = -1
    else:
        for a, b in zip(interp.batches, alt.batches):
            ea = effective(interp.materials[a["material"]])
            eb = effective(alt.materials[b["material"]])
            if ea != eb or [v["uv"] for v in a["verts"]] != [v["uv"] for v in b["verts"]]:
                diff += 1
    if diff:
        raise SystemExit(f"bg_{stem}: the export depends on the assumed starting RDP state ({diff} batches differ)")

    batches = flatten(interp, node_room, batch_extra)
    used = sorted({m["texture"]["id"] for m in interp.materials if m["texture"]})

    room_nodes = {}
    for i, n in enumerate(nodes):
        if n["type"] == "dl":
            room_nodes[(n["room"], n["layer"])] = i
    room_bb = {}
    for b in batches:
        r = nodes[b["node"]]["room"]
        lo, hi = room_bb.setdefault(r, ([float("inf")] * 3, [float("-inf")] * 3))
        for v in b["verts"]:
            for k in range(3):
                lo[k] = min(lo[k], v[k])
                hi[k] = max(hi[k], v[k])
    out_rooms = []
    for room in rooms:
        g = room.data
        # dlights.c:1665 reads vertices[i].flags for COLOUR index i, even past
        # the vertex array if there are more colours than vertices; mirrored.
        alpha_only = [i for i in range(room.numcolours)
                      if room.vertices + i * VTX_SIZE + 6 < len(g) and g[room.vertices + i * VTX_SIZE + 6] & 1]
        lo, hi = room_bb.get(room.num, (None, None))
        out_rooms.append({
            "room": room.num, "pos": list(room.pos), "bbmin": lo, "bbmax": hi,
            "br_light_min": room.br_light_min, "br_light_max": room.br_light_max,
            "opa_node": room_nodes.get((room.num, "opa")), "xlu_node": room_nodes.get((room.num, "xlu")),
            "numvertices": room.numvertices, "numcolours": room.numcolours,
            "colour_alpha_only": alpha_only, "bsp_parents": room.parents,
        })

    ntri = sum(len(b["indices"]) // 3 for b in batches)
    lo = [min(r["bbmin"][k] for r in out_rooms if r["bbmin"]) for k in range(3)]
    hi = [max(r["bbmax"][k] for r in out_rooms if r["bbmax"]) for k in range(3)]
    tlo, thi, _, _ = tiles_bbox(stem)
    summary = [
        f"bg_{stem}: rooms 1..{bg.roomcount - 1}, triangles {ntri}, batches {len(batches)}, "
        f"materials {len(interp.materials)}, textures {len(used)}",
        f"  env: fog={fog} transparency={transparency} ({env_src}); {replaced} commands rewritten",
        f"  bbox BG {[round(x, 1) for x in lo]}..{[round(x, 1) for x in hi]}, tiles {tlo}..{thi}",
    ]
    return {
        "name": f"bg_{stem}",
        "source": decomp_rel(path),
        "stage": stage_name,
        "units": "cm (world; room pos added, stage scale 1)",
        "env": {"fog": fog, "transparency": transparency, "provenance": env_src, "replaced_commands": replaced,
                "nofogenvironment": env_row},
        "nummatrices": 1,
        "nodes": nodes,
        "parts": {},
        "materials": interp.materials,
        "batches": batches,
        "rooms": out_rooms,
        "section2_textures": bg.section2_textures,
        "used": used,
        "warnings": warnings,
        "summary": summary,
    }


def used_textures(stem: str) -> list[int]:
    """The global texture numbers stage `stem`'s BG draws."""
    return build(stem)["used"]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("stage", choices=sorted(STAGES))
    args = ap.parse_args()
    d = build(args.stage)
    for line in d["summary"]:
        print(line)
    for w in d["warnings"]:
        print(f"  WARN {w}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
