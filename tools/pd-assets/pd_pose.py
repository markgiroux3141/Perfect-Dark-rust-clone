#!/usr/bin/env python3
"""Pose a Perfect Dark character model with a PD animation and export OBJ.

This is the proof that PD characters are fully portable: it takes a chr model
(`pd_model.py`) plus an animation (`pd_anim.py`), assembles the skeleton, and
writes a posed mesh.

# Why there is no separate "bind pose" to find

The bone hierarchy IS the model's node tree. `POSITION` nodes (type 0x02) are
joints; each carries a rest offset (`struct coord pos`) and a `part` number, and
`CHRINFO` (0x01) is the root joint with an `animpart`. Mesh (`DL`) nodes hang off
whichever joint is their nearest ancestor.

The animation then supplies, per part per frame, a rotation and (rarely) an extra
translation. Per `model_update_position_node_mtx` (`lib/model.c:1052`):

    local = rotation(anim_rot)                 with translation row set to
            node.pos  +  anim_translate * animscale   (or just node.pos when the
                                                       animation has no translation
                                                       channel for that part)
    world = local x parent_world               (row-vector convention, mtx_c.c:107)

So a rest pose is just "any animation, frame 0" — there is no missing asset. The
splayed figure you get from concatenating a chr's raw vertices is simply the
unposed state, because every bone's *rotation* lives in the animation data.

# Coordinate conventions

Rotations are Euler XYZ built exactly as `mtx4_load_rotation` (`lib/mtx.c:187`)
does, kept in that form rather than "simplified" so the result can be diffed
against the original. Matrices are row-major with the translation in row 3, and
vectors are rows (`v' = v @ M`), matching the N64 `Mtxf` layout.

Usage:
    python pd_pose.py <chr.bin> <ANIM_ID> [frame] [-o out.obj]
    python pd_pose.py --skeleton <chr.bin>          # print the bone tree
"""

from __future__ import annotations

import argparse
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from pd_anim import load_animation  # noqa: E402
from pd_model import ModelDef, load, seg_off, seg_ok  # noqa: E402

NODE_CHRINFO = 0x01
NODE_POSITION = 0x02
NODE_DL = 0x18
NODE_GUNDL = 0x04

# PD model units per real-world metre.
#
# Derived, not fitted. Two facts pin it down:
#
#  1. The engine scales a body by `g_HeadsAndBodies[bodynum].scale * 0.1`
#     (`body_instantiate_model_to_addr`, game/body.c:170), and `scale` is 1 for
#     ordinary bodies — so model units are multiplied by 0.1 to reach world units.
#  2. PD world units are centimetres. The same table gives each character a
#     `height` in cm (167, 165, 159...), and the gameplay constants agree: melee
#     range is 210 (2.1 m), a bot follows within 300 (3 m), and it decelerates
#     inside 200 of its destination (2 m).
#
# So model_raw * 0.1 = centimetres, i.e. 1000 model units per metre. Checks out
# against the models: a51guard measures 1725 raw = 1.73 m against a nominal 167 cm
# (mesh extents run slightly past nominal height, as you'd expect from hair and
# boots), mrblonde 1.77 m, and maian_soldier 0.99 m because Maians are a metre tall.
#
# Do NOT use `modeldef.scale` instead. It approximates the right value for many
# characters, which makes it look correct, but it is not what the engine uses and
# some models are wild outliers — `dark_frock` carries 1982.
UNITS_PER_METRE = 1000.0


# ---------------------------------------------------------------------------
# Matrix helpers — row-vector convention, translation in row 3
# ---------------------------------------------------------------------------


def identity() -> list[list[float]]:
    return [[1.0, 0, 0, 0], [0, 1.0, 0, 0], [0, 0, 1.0, 0], [0, 0, 0, 1.0]]


def rotation_matrix(rx: float, ry: float, rz: float) -> list[list[float]]:
    """`mtx4_load_rotation` (lib/mtx.c:187), transcribed term for term."""
    xcos, xsin = math.cos(rx), math.sin(rx)
    ycos, ysin = math.cos(ry), math.sin(ry)
    zcos, zsin = math.cos(rz), math.sin(rz)
    a = xsin * zsin
    b = xcos * zsin
    c = xsin * zcos
    d = xcos * zcos
    m = identity()
    m[0][0] = ycos * zcos
    m[0][1] = ycos * zsin
    m[0][2] = -ysin
    m[1][0] = c * ysin - xcos * zsin
    m[1][1] = a * ysin + xcos * zcos
    m[1][2] = xsin * ycos
    m[2][0] = d * ysin + xsin * zsin
    m[2][1] = b * ysin - xsin * zcos
    m[2][2] = xcos * ycos
    return m


def mul(local: list[list[float]], parent: list[list[float]]) -> list[list[float]]:
    """`mtx00015be4(parent, local, dst)` (lib/mtx_c.c:107) — dst = local x parent."""
    out = identity()
    for i in range(3):
        for j in range(3):
            out[i][j] = (
                parent[0][j] * local[i][0]
                + parent[1][j] * local[i][1]
                + parent[2][j] * local[i][2]
            )
        out[i][3] = 0.0
    for j in range(3):
        out[3][j] = (
            parent[0][j] * local[3][0]
            + parent[1][j] * local[3][1]
            + parent[2][j] * local[3][2]
            + parent[3][j]
        )
    out[3][3] = 1.0
    return out


def transform(v: tuple[float, float, float], m: list[list[float]]):
    x, y, z = v
    return (
        x * m[0][0] + y * m[1][0] + z * m[2][0] + m[3][0],
        x * m[0][1] + y * m[1][1] + z * m[2][1] + m[3][1],
        x * m[0][2] + y * m[1][2] + z * m[2][2] + m[3][2],
    )


# ---------------------------------------------------------------------------
# Skeleton
# ---------------------------------------------------------------------------


class Joint:
    __slots__ = ("offset", "part", "pos", "parent", "kind", "mtx", "slots")

    def __init__(self, offset, part, pos, parent, kind, mtx=(), slots=(-1, -1, -1)):
        self.offset = offset
        self.part = part
        self.pos = pos
        self.parent = parent  # parent joint's node offset, or None
        self.kind = kind
        # Matrix slots this joint owns. A POSITION node has three
        # (`mtxindexes[3]`); index 0 is the joint proper and 1/2 are blend
        # matrices the engine slerps for smooth skinning across the joint. We map
        # all three to this joint, which is exact for index 0 and a reasonable
        # approximation for the blends.
        self.mtx = mtx
        # The same three slots kept *positionally* (`-1` where absent), so a
        # consumer that treats slot 0 differently from the blends can tell them
        # apart. `pd_gltf.py` needs this: it gives each blend slot a real glTF
        # joint carrying the half-rotation, rather than folding it into slot 0.
        self.slots = slots


def build_skeleton(m: ModelDef) -> dict[int, Joint]:
    """Collect every joint node, each linked to its nearest joint ancestor."""
    nodes = {n.offset: n for n in m.walk()}

    def nearest_joint_ancestor(node):
        cur = node
        while seg_ok(cur.parent):
            p = nodes.get(seg_off(cur.parent))
            if p is None:
                return None
            if (p.type & 0xFF) in (NODE_POSITION, NODE_CHRINFO):
                return p.offset
            cur = p
        return None

    joints: dict[int, Joint] = {}
    for node in nodes.values():
        t = node.type & 0xFF
        if t == NODE_POSITION and seg_ok(node.rodata):
            ro = seg_off(node.rodata)
            x, y, z, part, mi0, mi1, mi2 = struct.unpack_from(">fffHhhh", m.data, ro)
            joints[node.offset] = Joint(
                node.offset, part, (x, y, z), nearest_joint_ancestor(node), "POSITION",
                tuple(i for i in (mi0, mi1, mi2) if i >= 0),
                (mi0, mi1, mi2),
            )
        elif t == NODE_CHRINFO and seg_ok(node.rodata):
            ro = seg_off(node.rodata)
            animpart, mtxindex = struct.unpack_from(">Hh", m.data, ro)
            joints[node.offset] = Joint(
                node.offset, animpart, (0.0, 0.0, 0.0), nearest_joint_ancestor(node), "CHRINFO",
                (mtxindex,) if mtxindex >= 0 else (),
                (mtxindex, -1, -1),
            )
    return joints


def matrix_to_joint(joints: dict[int, Joint]) -> dict[int, int]:
    """Map each matrix slot index to the joint node that owns it.

    This is what turns a vertex's `G_MTX` binding into a bone. Without it every
    vertex in a mesh node gets the node's own joint, which tears limbs apart at
    exactly the seams the blend matrices exist to hide.
    """
    out: dict[int, int] = {}
    for off, j in joints.items():
        for mi in j.mtx:
            out.setdefault(mi, off)
    return out


def joint_for_mesh(m: ModelDef, nodes: dict, mesh_offset: int):
    """The joint a mesh node is skinned to: its nearest joint ancestor."""
    cur = nodes[mesh_offset]
    while seg_ok(cur.parent):
        p = nodes.get(seg_off(cur.parent))
        if p is None:
            return None
        if (p.type & 0xFF) in (NODE_POSITION, NODE_CHRINFO):
            return p.offset
        cur = p
    return None


def pose_skeleton(m: ModelDef, joints: dict[int, Joint], anim, framenum: int):
    """Resolve every joint's world matrix for one animation frame."""
    world: dict[int, list[list[float]]] = {}

    def resolve(off: int):
        if off in world:
            return world[off]
        j = joints[off]
        rot, tra = ((0.0, 0.0, 0.0), (0.0, 0.0, 0.0))
        if anim is not None:
            rot, tra = anim.part_transform(j.part, framenum)
        local = rotation_matrix(*rot)
        # model.c:1158 — the animation translation is ADDED to the node's rest
        # offset, and only when the animation actually has one.
        if tra != (0.0, 0.0, 0.0):
            local[3][0] = j.pos[0] + tra[0]
            local[3][1] = j.pos[1] + tra[1]
            local[3][2] = j.pos[2] + tra[2]
        else:
            local[3][0], local[3][1], local[3][2] = j.pos
        parent = resolve(j.parent) if j.parent is not None and j.parent in joints else identity()
        world[off] = mul(local, parent)
        return world[off]

    for off in joints:
        resolve(off)
    return world


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------


def cmd_skeleton(path: str) -> int:
    m = load(path)
    joints = build_skeleton(m)
    order = sorted(joints.values(), key=lambda j: j.offset)
    print(f"{m.name}: {len(joints)} joints, model scale {m.scale:.1f}")
    depth_of: dict[int, int] = {}
    for j in order:
        d = 0
        p = j.parent
        while p is not None and p in joints:
            d += 1
            p = joints[p].parent
        depth_of[j.offset] = d
    for j in order:
        indent = "  " * depth_of[j.offset]
        print(
            f"  {indent}part {j.part:<3} {j.kind:<8} "
            f"offset=({j.pos[0]:8.1f},{j.pos[1]:8.1f},{j.pos[2]:8.1f})"
        )
    return 0


def cmd_pose(path: str, anim_id: str | None, framenum: int, out: str,
             units_per_metre: float = UNITS_PER_METRE) -> int:
    m = load(path)
    joints = build_skeleton(m)
    nodes = {n.offset: n for n in m.walk()}
    anim = load_animation(anim_id) if anim_id else None
    if anim is not None and framenum >= anim.numframes:
        raise SystemExit(f"{anim.id} has {anim.numframes} frames; {framenum} is out of range")

    world = pose_skeleton(m, joints, anim, framenum)
    groups = m.geometry()
    scale = units_per_metre

    mtx_map = matrix_to_joint(joints)
    verts_out: list[tuple[float, float, float]] = []
    faces_out: list[tuple[int, int, int]] = []
    skinned = unskinned = 0

    for g in groups:
        # Fallback for any vertex whose G_MTX slot we can't resolve: the mesh
        # node's own nearest joint ancestor.
        fallback_joint = joint_for_mesh(m, nodes, g.node_offset)
        base = len(verts_out)
        for v in g.verts:
            jo = mtx_map.get(v.mtx, fallback_joint)
            mtx = world.get(jo) if jo is not None else None
            if mtx is None:
                mtx = identity()
                unskinned += 1
            else:
                skinned += 1
            p = transform((float(v.x), float(v.y), float(v.z)), mtx)
            verts_out.append((p[0] / scale, p[1] / scale, p[2] / scale))
        for a, b, c, _tex in g.tris:
            faces_out.append((base + a + 1, base + b + 1, base + c + 1))

    label = f"{anim.id} frame {framenum}" if anim else "rest (no animation)"
    mtl_name = os.path.splitext(os.path.basename(out))[0] + ".mtl"

    # A distinct muted colour per mesh group. Untextured, this would render as one
    # white blob in which a wrong bone assignment is invisible; per-part colour makes
    # a mis-parented limb obvious at a glance, which is the whole point of eyeballing
    # it. The palette is deliberately desaturated so the silhouette still reads as a
    # figure rather than a test pattern.
    palette = [
        (0.72, 0.68, 0.62), (0.45, 0.50, 0.58), (0.62, 0.48, 0.42),
        (0.50, 0.58, 0.48), (0.66, 0.60, 0.45), (0.42, 0.46, 0.56),
        (0.70, 0.55, 0.50), (0.52, 0.56, 0.62),
    ]
    with open(os.path.join(os.path.dirname(out) or ".", mtl_name), "w", encoding="utf-8") as fh:
        fh.write(f"# per-part debug palette for {m.name}\n")
        for i in range(len(groups)):
            r, g, b = palette[i % len(palette)]
            fh.write(f"newmtl part{i:03d}\nKd {r:.3f} {g:.3f} {b:.3f}\nKa 0 0 0\nd 1\nillum 1\n\n")

    with open(out, "w", encoding="utf-8") as fh:
        fh.write(f"# {m.name} posed with {label}\n")
        fh.write(f"mtllib {mtl_name}\n")
        for v in verts_out:
            fh.write(f"v {v[0]:.6f} {v[1]:.6f} {v[2]:.6f}\n")
        # Faces are emitted per group so each can carry its own material.
        fi = 0
        for i, g in enumerate(groups):
            fh.write(f"usemtl part{i:03d}\n")
            for _ in g.tris:
                f = faces_out[fi]
                fh.write(f"f {f[0]} {f[1]} {f[2]}\n")
                fi += 1

    xs = [v[0] for v in verts_out]
    ys = [v[1] for v in verts_out]
    zs = [v[2] for v in verts_out]
    print(f"{m.name} + {label} -> {out}")
    print(f"  {len(groups)} mesh groups, {len(verts_out)} verts ({skinned} skinned, "
          f"{unskinned} unbound), {len(faces_out)} tris, {len(mtx_map)} matrix slots mapped")
    print(f"  bounds (m)  X {max(xs) - min(xs):6.3f}   Y {max(ys) - min(ys):6.3f}   "
          f"Z {max(zs) - min(zs):6.3f}   [at {units_per_metre:.0f} units/m]")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("model")
    ap.add_argument("anim", nargs="?")
    ap.add_argument("frame", nargs="?", type=int, default=0)
    ap.add_argument("-o", "--out", default=None)
    ap.add_argument("--skeleton", action="store_true", help="print the bone tree and exit")
    ap.add_argument("--units-per-metre", type=float, default=UNITS_PER_METRE)
    args = ap.parse_args()

    if args.skeleton:
        return cmd_skeleton(args.model)
    out = args.out or os.path.splitext(os.path.basename(args.model))[0] + "_posed.obj"
    return cmd_pose(args.model, args.anim, args.frame, out, args.units_per_metre)


if __name__ == "__main__":
    sys.exit(main())
