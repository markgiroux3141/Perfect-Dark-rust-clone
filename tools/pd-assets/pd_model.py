#!/usr/bin/env python3
"""Perfect Dark model (.bin) -> OBJ converter / inspector.

Feasibility prototype for pulling PD geometry into the native engine. Parses the
raw N64 model segments extracted by `pd-decomp/tools/extract` (see START_HERE.md
section 2) and walks them the same way `lib/model.c` does at load time.

Format facts, all confirmed against the decompilation:

* A model file IS a `struct modeldef` at offset 0, followed by its node tree,
  vertex arrays and display lists. Every pointer inside is a *segmented address*
  based at VMA 0x05000000, so `file_offset = addr & 0xffffff`
  (`model_promote_offsets_to_pointers`, lib/model.c:3968).
* `modeldef.skel` is NOT a pointer when under 0x10000 -- it is an index into the
  shared skeleton table, resolved at load (`model_promote_type_to_pointer`,
  game/modeldef.c:167). PD character models share a handful of skeletons.
* Rare shipped a *custom microcode*, so this is not stock F3DEX:
    - `Vtx` is 12 bytes, not 16: s16 x,y,z / u8 flags / u8 colour / s16 s,t.
      `colour` indexes the model's own colour table (include/PR/gbi.h:980).
    - `G_TRI4` (0xb1) packs up to FOUR triangles into one 8-byte command with
      4-bit vertex indices; an all-zero triangle is a skipped slot
      (src/include/gbiex.h:22).
  Everything else (G_VTX=0x04, G_DL=0x06, G_ENDDL=0xb8, G_SETTIMG=0xfd) is
  ordinary F3DEX.
* Addresses *inside a display list* are not all segment 5. The renderer rebinds
  the RSP segment table per drawn node (`lib/model.c:3234`, constants.h:3919):
      segment 3 = matrices
      segment 4 = this node's own vertex array   <- what G_VTX reads
      segment 5 = COL1, which the loader points at the model base, so this is
                  the plain "file offset" segment
      segment 6 = COL2, the colour array that follows the vertices
  Reading G_VTX as a segment-5 offset is the trap: it silently yields garbage
  geometry for character and prop models (guns happen to survive it).

Usage:
    python pd_model.py info  <model.bin>
    python pd_model.py obj   <model.bin> <out.obj>
    python pd_model.py batch <indir> <outdir> [--limit N]
"""

from __future__ import annotations

import argparse
import os
import struct
import sys
from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# Segmented addressing
# ---------------------------------------------------------------------------

SEGMENT_BASE = 0x05000000


def seg_ok(addr: int) -> bool:
    """True if `addr` is a segment-05 pointer (what every in-file pointer is)."""
    return (addr >> 24) == 0x05


def seg_num(addr: int) -> int:
    return addr >> 24


def seg_off(addr: int) -> int:
    return addr & 0xFFFFFF


# ---------------------------------------------------------------------------
# Node types (src/include/constants.h:2267)
# ---------------------------------------------------------------------------

NODETYPE = {
    0x01: "CHRINFO",
    0x02: "POSITION",
    0x04: "GUNDL",
    0x05: "TYPE05",
    0x08: "DISTANCE",
    0x09: "REORDER",
    0x0A: "BBOX",
    0x0B: "TYPE0B",
    0x0C: "CHRGUNFIRE",
    0x0D: "TYPE0D",
    0x0E: "TYPE0E",
    0x0F: "TYPE0F",
    0x11: "TYPE11",
    0x12: "TOGGLE",
    0x15: "POSITIONHELD",
    0x16: "STARGUNFIRE",
    0x17: "HEADSPOT",
    0x18: "DL",
}

# GBI opcodes we care about.
G_MTX = 0x01
G_VTX = 0x04
G_DL = 0x06
G_TRI4 = 0xB1
G_ENDDL = 0xB8
# Stock F3DEX single triangle. PD emits BOTH this and its custom G_TRI4 — the
# guard model alone has 112 of these against 127 TRI4 commands — so handling only
# the exotic one leaves the mesh full of holes.
G_TRI1 = 0xBF
G_SETTIMG = 0xFD
# PD's own "bind texture" command, used instead of `G_SETTIMG` by every model whose
# textures live in the global pool rather than inline. `w1` is the texture number,
# which the game rewrites into a real `G_SETTIMG` + `0xabcdXXXX` word at load so
# `tex_load_from_display_list` can find and inflate it (texdecompress.c:2090).
#
# Not guessed: across the 144 character models that emit it, **all 5,474 operands**
# are a key in that model's own `texconfigs` table. Without it, a pooled-texture
# model reports no textures at all and silently falls back to flat colour.
G_SETTEXNUM = 0xC0

VTX_SIZE = 12
GFX_SIZE = 8
# Size of one N64 `Mtx` in bytes, so a segment-3 byte offset divides down to a
# matrix index.
MTX_SIZE = 64
# RSP segment holding the model's matrix array (SPSEGMENT_MODEL_MTX, constants.h:3919).
SEG_MTX = 0x03

MAX_DL_DEPTH = 8  # display lists nest shallowly; a guard against cyclic data


# ---------------------------------------------------------------------------
# Parsed structures
# ---------------------------------------------------------------------------


@dataclass
class Vertex:
    x: int
    y: int
    z: int
    flags: int
    colour: int
    s: int
    t: int
    # Index into the model's matrix array that was bound by the most recent G_MTX
    # when this vertex was loaded — i.e. which bone it belongs to. See the
    # `G_MTX` handling in `_run_dl`.
    mtx: int = -1


@dataclass
class Node:
    offset: int
    type: int
    rodata: int
    parent: int
    next: int
    prev: int
    child: int

    @property
    def typename(self) -> str:
        return NODETYPE.get(self.type & 0xFF, f"UNK_{self.type:#04x}")


@dataclass
class TexConfig:
    texturenum: int
    width: int
    height: int


@dataclass
class Group:
    """One drawable node's geometry, kept separate so parts stay identifiable."""

    node_offset: int
    node_type: str
    verts: list[Vertex] = field(default_factory=list)
    # (i0, i1, i2, texturenum) indices into `verts`
    tris: list[tuple[int, int, int, int]] = field(default_factory=list)


class ModelDef:
    def __init__(self, data: bytes, name: str = "?"):
        self.data = data
        self.name = name
        if len(data) < 0x1C:
            raise ValueError(f"{name}: too small to be a modeldef ({len(data)} bytes)")
        (
            self.rootnode,
            self.skel,
            self.parts,
            self.numparts,
            self.nummatrices,
            self.scale,
            self.rwdatalen,
            self.numtexconfigs,
            self.texconfigs,
        ) = struct.unpack_from(">IIIhhfhhI", data, 0)

        if not seg_ok(self.rootnode):
            raise ValueError(
                f"{name}: rootnode {self.rootnode:#010x} is not a segment-05 pointer "
                "-- not a model file?"
            )

    # -- primitive reads ---------------------------------------------------

    def u32(self, off: int) -> int:
        return struct.unpack_from(">I", self.data, off)[0]

    def read_node(self, off: int) -> Node:
        t, rodata, parent, nxt, prev, child = struct.unpack_from(">HxxIIIII", self.data, off)
        return Node(off, t, rodata, parent, nxt, prev, child)

    # -- tree walk ---------------------------------------------------------

    def walk(self) -> list[Node]:
        """Depth-first walk of the node tree, mirroring model.c's iteration."""
        out: list[Node] = []
        seen: set[int] = set()
        off = seg_off(self.rootnode)
        while off:
            if off in seen or off + 0x18 > len(self.data):
                break
            seen.add(off)
            node = self.read_node(off)
            out.append(node)

            # DISTANCE nodes retarget child at load time (model.c:3903).
            child = node.child
            if (node.type & 0xFF) == 0x08 and seg_ok(node.rodata):
                target = self.u32(seg_off(node.rodata) + 8)
                if seg_ok(target):
                    child = target

            if seg_ok(child) and seg_off(child) not in seen:
                off = seg_off(child)
                continue
            # Ascend until a sibling is available.
            cur = node
            while True:
                if seg_ok(cur.next) and seg_off(cur.next) not in seen:
                    off = seg_off(cur.next)
                    break
                if not seg_ok(cur.parent):
                    off = 0
                    break
                cur = self.read_node(seg_off(cur.parent))
        return out

    def texture_configs(self) -> list[TexConfig]:
        out: list[TexConfig] = []
        if not (self.numtexconfigs and seg_ok(self.texconfigs)):
            return out
        base = seg_off(self.texconfigs)
        # struct textureconfig is 12 bytes: void *texturenum; u8 width, height,
        # level, s, t, x, y, unk0b.
        for i in range(self.numtexconfigs):
            off = base + i * 12
            if off + 12 > len(self.data):
                break
            num = struct.unpack_from(">I", self.data, off)[0]
            w, h = struct.unpack_from(">BB", self.data, off + 4)
            out.append(TexConfig(num, w, h))
        return out

    # -- geometry ----------------------------------------------------------

    def _read_vertices(self, addr: int, count: int) -> list[Vertex]:
        out: list[Vertex] = []
        base = seg_off(addr)
        for i in range(count):
            off = base + i * VTX_SIZE
            if off + VTX_SIZE > len(self.data):
                break
            x, y, z, flags, colour, s, t = struct.unpack_from(">hhhBBhh", self.data, off)
            out.append(Vertex(x, y, z, flags, colour, s, t))
        return out

    def _resolve(self, addr: int, segs: dict[int, int]) -> int | None:
        """Resolve a segmented address to a file offset using this node's
        segment bindings, or None if the segment isn't mapped."""
        base = segs.get(seg_num(addr))
        if base is None:
            return None
        off = base + seg_off(addr)
        return off if 0 <= off < len(self.data) else None

    def _run_dl(self, addr: int, group: Group, segs: dict[int, int], depth: int = 0) -> None:
        """Execute a display list, appending triangles to `group`.

        Only the commands that carry geometry are interpreted; render-state
        commands are skipped, except G_SETTIMG which we track so each triangle
        remembers which texture it wanted. `segs` maps RSP segment number to a
        file offset (see the module docstring).
        """
        start = self._resolve(addr, segs)
        if depth > MAX_DL_DEPTH or start is None:
            return
        off = start
        # The RSP vertex buffer: 16 slots, holding indices into group.verts.
        vbuf: list[int | None] = [None] * 16
        cur_tex = -1
        # The bone currently bound by G_MTX. A single display list switches this
        # mid-stream, so one mesh node can span several bones — a thigh mesh
        # covering both hip and knee, for instance. Tracking it per vertex is what
        # makes skinning work; assuming one bone per mesh node tears the model apart
        # at every joint.
        cur_mtx = -1

        while off + GFX_SIZE <= len(self.data):
            w0, w1 = struct.unpack_from(">II", self.data, off)
            off += GFX_SIZE
            op = w0 >> 24

            if op == G_ENDDL:
                return

            if op == G_MTX:
                # Segment 3 is bound to the whole matrix array (`gSPSegment(...,
                # SPSEGMENT_MODEL_MTX, model->matrices)`, lib/model.c:3525), so the
                # byte offset divides down to a bone index.
                if seg_num(w1) == SEG_MTX:
                    cur_mtx = seg_off(w1) // MTX_SIZE
                continue

            if op == G_VTX:
                # gDma1p: w0 = op<<24 | p<<16 | len ; p = (n-1)<<4 | v0
                p = (w0 >> 16) & 0xFF
                n = ((p >> 4) & 0xF) + 1
                v0 = p & 0xF
                src = self._resolve(w1, segs)
                if src is None:
                    continue
                verts = self._read_vertices(src, n)
                for i, v in enumerate(verts):
                    if v0 + i < 16:
                        v.mtx = cur_mtx
                        vbuf[v0 + i] = len(group.verts)
                        group.verts.append(v)
                continue

            if op == G_TRI4:
                # gbiex.h:22 -- four triangles, 4-bit indices, all-zero = unused.
                zs = [(w0 >> (4 * i)) & 0xF for i in range(4)]
                xs = [(w1 >> (8 * i)) & 0xF for i in range(4)]
                ys = [(w1 >> (8 * i + 4)) & 0xF for i in range(4)]
                for i in range(4):
                    a, b, c = xs[i], ys[i], zs[i]
                    if a == 0 and b == 0 and c == 0:
                        continue
                    ia, ib, ic = vbuf[a], vbuf[b], vbuf[c]
                    if ia is None or ib is None or ic is None:
                        continue
                    group.tris.append((ia, ib, ic, cur_tex))
                continue

            if op == G_TRI1:
                # `__gsSP1Triangle_w1f` (gbi.h:1748): indices are stored x10 (the
                # DMEM vertex stride), one per byte of w1, with the flag in the top
                # byte. Unlike G_TRI4 there is no "all zero = unused slot" rule —
                # (0, 1, 2) is a perfectly ordinary triangle here.
                ia = vbuf[((w1 >> 16) & 0xFF) // 10]
                ib = vbuf[((w1 >> 8) & 0xFF) // 10]
                ic = vbuf[(w1 & 0xFF) // 10]
                if ia is not None and ib is not None and ic is not None:
                    group.tris.append((ia, ib, ic, cur_tex))
                continue

            if op == G_SETTIMG or op == G_SETTEXNUM:
                # Both name a `texconfigs` entry: `G_SETTIMG` by segmented address
                # (inline textures), `G_SETTEXNUM` by pool texture number.
                cur_tex = w1
                continue

            if op == G_DL:
                # w0 bit 16 set = branch (no return), clear = call.
                branch = (w0 >> 16) & 0xFF
                if branch:
                    nxt = self._resolve(w1, segs)
                    if nxt is None:
                        return
                    off = nxt
                    continue
                self._run_dl(w1, group, segs, depth + 1)
                continue

            # Anything else is render state -- ignore.

    def geometry(self) -> list[Group]:
        """Extract one Group per drawable node (DL / GUNDL)."""
        groups: list[Group] = []
        for node in self.walk():
            t = node.type & 0xFF
            if t not in (0x18, 0x04) or not seg_ok(node.rodata):
                continue
            ro = seg_off(node.rodata)
            if ro + 0x18 > len(self.data):
                continue
            opagdl, xlugdl = struct.unpack_from(">II", self.data, ro)
            if t == 0x18:  # modelrodata_dl: opa, xlu, colours, vertices, numvertices
                vtx_addr, numvertices = struct.unpack_from(">Ih", self.data, ro + 0x0C)
            else:  # modelrodata_gundl: opa, xlu, baseaddr, vertices, numvertices
                vtx_addr, numvertices = struct.unpack_from(">Ih", self.data, ro + 0x0C)
            if not seg_ok(vtx_addr):
                continue
            vtx_base = seg_off(vtx_addr)
            # The colour array immediately follows the vertices (see the comment
            # on modelrodata_dl.vertices in types.h).
            segs = {
                0x04: vtx_base,
                0x05: 0,
                0x06: vtx_base + max(numvertices, 0) * VTX_SIZE,
            }
            group = Group(node.offset, node.typename)
            for gdl in (opagdl, xlugdl):
                if gdl:
                    self._run_dl(gdl, group, segs)
            if group.tris:
                groups.append(group)
        return groups


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------


def load(path: str) -> ModelDef:
    with open(path, "rb") as fh:
        return ModelDef(fh.read(), os.path.basename(path))


def cmd_info(path: str) -> int:
    m = load(path)
    nodes = m.walk()
    counts: dict[str, int] = {}
    for n in nodes:
        counts[n.typename] = counts.get(n.typename, 0) + 1
    groups = m.geometry()
    nverts = sum(len(g.verts) for g in groups)
    ntris = sum(len(g.tris) for g in groups)

    print(f"{m.name}: {len(m.data)} bytes")
    print(f"  skeleton id   : {m.skel}" + ("  (shared skeleton table)" if m.skel < 0x10000 else ""))
    print(f"  parts         : {m.numparts}")
    print(f"  matrices/bones: {m.nummatrices}")
    print(f"  scale         : {m.scale}")
    print(f"  texconfigs    : {m.numtexconfigs}")
    print(f"  nodes         : {len(nodes)}")
    for name in sorted(counts):
        print(f"      {name:<14} {counts[name]}")
    print(f"  geometry      : {len(groups)} groups, {nverts} verts, {ntris} tris")
    tcs = m.texture_configs()
    if tcs:
        preview = ", ".join(f"{t.texturenum:#x}({t.width}x{t.height})" for t in tcs[:6])
        print(f"  textures      : {preview}{' ...' if len(tcs) > 6 else ''}")
    return 0


def cmd_obj(path: str, out: str) -> int:
    m = load(path)
    groups = m.geometry()
    if not groups:
        print(f"{m.name}: no geometry found", file=sys.stderr)
        return 1

    # PD stores positions as s16 in model units; modeldef.scale is the divisor
    # used to bring them into world units (model.c applies it as a matrix scale).
    scale = m.scale if m.scale else 1.0
    nverts = ntris = 0
    with open(out, "w", encoding="utf-8") as fh:
        fh.write(f"# converted from Perfect Dark model {m.name}\n")
        fh.write(f"# skeleton={m.skel} parts={m.numparts} bones={m.nummatrices}\n")
        base = 1  # OBJ indices are 1-based and global across groups
        for gi, g in enumerate(groups):
            fh.write(f"g part{gi:03d}_{g.node_type}_{g.node_offset:06x}\n")
            for v in g.verts:
                fh.write(f"v {v.x / scale:.6f} {v.y / scale:.6f} {v.z / scale:.6f}\n")
            for v in g.verts:
                # N64 texcoords are S10.5 fixed point.
                fh.write(f"vt {v.s / 32.0:.6f} {v.t / 32.0:.6f}\n")
            for a, b, c, _tex in g.tris:
                fa, fb, fc = base + a, base + b, base + c
                fh.write(f"f {fa}/{fa} {fb}/{fb} {fc}/{fc}\n")
            base += len(g.verts)
            nverts += len(g.verts)
            ntris += len(g.tris)
    print(f"{m.name} -> {out}: {len(groups)} groups, {nverts} verts, {ntris} tris")
    return 0


def cmd_batch(indir: str, outdir: str, limit: int | None) -> int:
    os.makedirs(outdir, exist_ok=True)
    names = sorted(n for n in os.listdir(indir) if n.endswith(".bin"))
    if limit:
        names = names[:limit]
    ok = failed = empty = 0
    for name in names:
        src = os.path.join(indir, name)
        dst = os.path.join(outdir, name[:-4] + ".obj")
        try:
            m = load(src)
            groups = m.geometry()
        except Exception as exc:  # noqa: BLE001 - survey tool, report and continue
            print(f"  FAIL {name}: {exc}")
            failed += 1
            continue
        if not groups:
            empty += 1
            continue
        cmd_obj(src, dst)
        ok += 1
    print(f"\n{ok} converted, {empty} with no geometry, {failed} failed, {len(names)} total")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("info", help="print structure of one model")
    p.add_argument("model")

    p = sub.add_parser("obj", help="convert one model to OBJ")
    p.add_argument("model")
    p.add_argument("out")

    p = sub.add_parser("batch", help="convert a directory of models")
    p.add_argument("indir")
    p.add_argument("outdir")
    p.add_argument("--limit", type=int, default=None)

    args = ap.parse_args()
    if args.cmd == "info":
        return cmd_info(args.model)
    if args.cmd == "obj":
        return cmd_obj(args.model, args.out)
    return cmd_batch(args.indir, args.outdir, args.limit)


if __name__ == "__main__":
    sys.exit(main())
