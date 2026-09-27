#!/usr/bin/env python3
"""Render a skinned GLB to a PNG, on the CPU, with no engine and no dependencies.

`pd_gltf.py` can be wrong in ways every structural check passes — the Perfect Dark
import has already produced two of them (per-mesh instead of per-vertex skinning,
and a dropped triangle opcode), both invisible to assertions about joint counts,
heights and symmetry, both obvious the moment the model was drawn. This exists so
that "put it on screen" does not require a build, a GPU or a human.

It deliberately shares **nothing** with the exporter but the PNG writer: it
re-reads the `.glb` off disk and re-implements the glTF skinning math
(`jointMatrix = global(joint) x inverseBind(joint)`, then linear blend skinning)
from the spec. A mistake would have to be made twice, in opposite directions, to
hide.

It is a general glTF reader, not a PD one — point it at
`assets/enemies/characters/*.glb` to see a GoldenEye body through the exact same
renderer, which is how a PD body's scale, facing and handedness get checked
against a known-good asset rather than against an opinion.

`--positions` goes one better and renders what the **engine** computed, from a dump
written by `cargo run --release --example pd_pose_dump`. That is the check this
script cannot make on its own: the exporter and this renderer could be
self-consistently wrong about the same asset, but they cannot both also agree with
an independent Rust implementation. (Measured across all four bodies x all four
clips: they agree to 0.0006 mm, i.e. float32 rounding.)

Usage:
    python pd_preview.py <model.glb> <out.png>
    python pd_preview.py <model.glb> <out.png> --clip <clip.glb> --frames 6
    python pd_preview.py <model.glb> <out.png> --yaw 90 --highlight Bone_9
    python pd_preview.py <model.glb> <out.png> --positions <dump.f32>
    python pd_preview.py <model.glb> <out.png> --frame-radius 1300   # compare sizes
"""

from __future__ import annotations

import argparse
import json
import math
import os
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from pd_gltf import png_bytes  # noqa: E402

#: How hard `--viewmodel` adds the matcap reflection, kept equal to the `1.6` in
#: `shader_viewmodel.wgsl`. If that constant is ever tuned, this follows it — the
#: whole point of the mode is that the preview is not a second opinion.
ENV_GAIN = 1.6

CT_SIZE = {5120: 1, 5121: 1, 5122: 2, 5123: 2, 5125: 4, 5126: 4}
CT_FMT = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


# ---------------------------------------------------------------------------
# 4x4 matrices, column-vector convention (v' = M @ v) — glTF's, and glam's
# ---------------------------------------------------------------------------


def m_identity():
    return [[1.0 if i == j else 0.0 for j in range(4)] for i in range(4)]


def m_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def m_point(m, p):
    x, y, z = p
    w = m[3][0] * x + m[3][1] * y + m[3][2] * z + m[3][3]
    w = w if abs(w) > 1e-12 else 1.0
    return (
        (m[0][0] * x + m[0][1] * y + m[0][2] * z + m[0][3]) / w,
        (m[1][0] * x + m[1][1] * y + m[1][2] * z + m[1][3]) / w,
        (m[2][0] * x + m[2][1] * y + m[2][2] * z + m[2][3]) / w,
    )


def m_from_trs(t, r, s):
    x, y, z, w = r
    xx, yy, zz = x * x, y * y, z * z
    m = [
        [1 - 2 * (yy + zz), 2 * (x * y - z * w), 2 * (x * z + y * w), t[0]],
        [2 * (x * y + z * w), 1 - 2 * (xx + zz), 2 * (y * z - x * w), t[1]],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (xx + yy), t[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
    for i in range(3):
        for j in range(3):
            m[i][j] *= s[j]
    return m


def quat_slerp(a, b, t):
    d = sum(a[i] * b[i] for i in range(4))
    if d < 0.0:
        b, d = [-v for v in b], -d
    if d > 0.9995:
        out = [a[i] + (b[i] - a[i]) * t for i in range(4)]
    else:
        th = math.acos(max(-1.0, min(1.0, d)))
        s = math.sin(th)
        wa, wb = math.sin((1 - t) * th) / s, math.sin(t * th) / s
        out = [a[i] * wa + b[i] * wb for i in range(4)]
    n = math.sqrt(sum(v * v for v in out)) or 1.0
    return [v / n for v in out]


# ---------------------------------------------------------------------------
# glTF reading
# ---------------------------------------------------------------------------


class Gltf:
    def __init__(self, path: str):
        raw = open(path, "rb").read()
        magic, _ver, _total = struct.unpack_from("<III", raw, 0)
        if magic != 0x46546C67:
            raise SystemExit(f"{path}: not a GLB")
        off, js, bn = 12, None, b""
        while off + 8 <= len(raw):
            ln, ctype = struct.unpack_from("<II", raw, off)
            data = raw[off + 8 : off + 8 + ln]
            if ctype == 0x4E4F534A:
                js = data
            elif ctype == 0x004E4942:
                bn = data
            off += 8 + ln
        self.doc = json.loads(js)
        self.bin = bn
        self.path = path

    def accessor(self, i: int):
        """Return an accessor's values as a list of per-element tuples."""
        acc = self.doc["accessors"][i]
        n = NCOMP[acc["type"]]
        ct = acc["componentType"]
        size = CT_SIZE[ct] * n
        view = self.doc["bufferViews"][acc["bufferView"]]
        base = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
        stride = view.get("byteStride") or size
        fmt = "<" + CT_FMT[ct] * n
        return [
            struct.unpack_from(fmt, self.bin, base + k * stride)
            for k in range(acc["count"])
        ]

    def node_parents(self):
        parent = {}
        for i, node in enumerate(self.doc.get("nodes", [])):
            for c in node.get("children", []):
                parent[c] = i
        return parent

    def node_trs(self, i: int):
        node = self.doc["nodes"][i]
        if "matrix" in node:
            m = node["matrix"]  # column-major
            return [[m[j * 4 + i] for j in range(4)] for i in range(4)]
        return m_from_trs(
            node.get("translation", [0, 0, 0]),
            node.get("rotation", [0, 0, 0, 1]),
            node.get("scale", [1, 1, 1]),
        )


class Model:
    """A skinned GLB: geometry, the skin's joints, and the per-primitive images."""

    def __init__(self, path: str):
        g = Gltf(path)
        self.g = g
        skins = g.doc.get("skins") or []
        if skins:
            skin = skins[0]
            self.joint_nodes = skin["joints"]
            self.names = [
                g.doc["nodes"][n].get("name", f"joint{i}") for i, n in enumerate(self.joint_nodes)
            ]
            pos_in_skin = {n: i for i, n in enumerate(self.joint_nodes)}
            parents = g.node_parents()
            self.parents = [pos_in_skin.get(parents.get(n, -1)) for n in self.joint_nodes]
            if "inverseBindMatrices" in skin:
                self.ibm = [
                    [[m[j * 4 + i] for j in range(4)] for i in range(4)]
                    for m in g.accessor(skin["inverseBindMatrices"])
                ]
            else:
                self.ibm = [m_identity() for _ in self.joint_nodes]
            self.bind = [g.node_trs(n) for n in self.joint_nodes]
            self.skinned = True
        else:
            # A STATIC mesh — the weapon exports (`pd_gltf.py gun`) are deliberately
            # unskinned, because our viewmodel is one mesh with a recoil kick and PD's
            # articulated gun parts have nowhere to land yet. Rather than a second code
            # path, stand in one identity joint that every vertex binds to with weight
            # 1: posing and skinning below then work untouched, so the static case is
            # drawn by exactly the same maths as the characters (which is the whole
            # point of this tool sharing nothing with the exporter).
            self.joint_nodes = []
            self.names = ["static"]
            self.parents = [None]
            self.ibm = [m_identity()]
            self.bind = [m_identity()]
            self.skinned = False

        self.verts: list[tuple] = []
        self.uvs: list[tuple] = []
        self.cols: list[tuple] = []
        self.nrms: list[tuple] = []
        self.jnts: list[tuple] = []
        self.wgts: list[tuple] = []
        self.tris: list[tuple[int, int, int, int]] = []  # (a, b, c, primitive)
        self.images: list[tuple[int, int, bytes] | None] = []
        self.factors: list[list[float]] = []
        self.env_prim: list[bool] = []
        for mesh in g.doc.get("meshes", []):
            for prim in mesh["primitives"]:
                attrs = prim["attributes"]
                base = len(self.verts)
                p = g.accessor(attrs["POSITION"])
                self.verts += p
                self.uvs += (
                    g.accessor(attrs["TEXCOORD_0"]) if "TEXCOORD_0" in attrs else [(0.0, 0.0)] * len(p)
                )
                # Vertex colours (glTF `COLOR_0`). The engine's viewmodel shader
                # multiplies these onto the sampled texel, and PD's guns carry their
                # whole shading here — so without them a preview shows a flat
                # silhouette and reports it as "no textures".
                self.cols += (
                    g.accessor(attrs["COLOR_0"])
                    if "COLOR_0" in attrs
                    else [(1.0, 1.0, 1.0, 1.0)] * len(p)
                )
                # `NORMAL` matters for `--viewmodel`: the engine's matcap reflection
                # samples the environment map by the raw (untransformed) normal.
                self.nrms += (
                    g.accessor(attrs["NORMAL"]) if "NORMAL" in attrs else [(0.0, 1.0, 0.0)] * len(p)
                )
                self.jnts += (
                    g.accessor(attrs["JOINTS_0"]) if "JOINTS_0" in attrs else [(0, 0, 0, 0)] * len(p)
                )
                self.wgts += (
                    g.accessor(attrs["WEIGHTS_0"])
                    if "WEIGHTS_0" in attrs
                    else [(1.0, 0.0, 0.0, 0.0)] * len(p)
                )
                idx = (
                    [v[0] for v in g.accessor(prim["indices"])]
                    if "indices" in prim
                    else list(range(len(p)))
                )
                pi = len(self.images)
                self.images.append(self._prim_image(prim))
                self.factors.append(self._prim_factor(prim))
                self.env_prim.append(self._prim_is_env(prim))
                for k in range(0, len(idx) - 2, 3):
                    self.tris.append((base + idx[k], base + idx[k + 1], base + idx[k + 2], pi))

    def env_image(self):
        """The model's environment/reflection map, or `None`.

        Exactly the rule `engine/src/assets/textured_model.rs` applies: the base
        texture of the FIRST material whose name contains `EnvMapping` — the
        editor's own render-intent tag. Reproducing the rule here (rather than
        approximating it) is the point: `--viewmodel` is for judging what the
        engine will draw, and half the PD arsenal carries such a material.
        """
        for i, mat in enumerate(self.g.doc.get("materials", [])):
            if "EnvMapping" not in (mat.get("name") or ""):
                continue
            for mesh in self.g.doc.get("meshes", []):
                for prim in mesh["primitives"]:
                    if prim.get("material") == i:
                        return self._prim_image(prim)
        return None

    def env_per_material(self) -> bool:
        """Whether the reflection covers only the `EnvMapping` primitives.

        The game's `combat::viewmodel::env_scope` makes this call by asset family
        (PD's exports live under `assets/weapons/pd/`); a preview is handed a bare
        path, so it reads the same fact off the file — `extras.pd_gun` is written
        by `pd_gltf.py`'s gun export and by nothing else. Same rule, same answer,
        from the only evidence available here.
        """
        return "pd_gun" in (self.g.doc.get("extras") or {})

    def _prim_is_env(self, prim) -> bool:
        try:
            return "EnvMapping" in (self.g.doc["materials"][prim["material"]].get("name") or "")
        except (KeyError, IndexError):
            return False

    def _prim_factor(self, prim):
        """`baseColorFactor`, which the engine folds into the vertex colour.

        Matters for the PD guns: an `EnvMapping` material carries a BLACK factor,
        because those faces are painted by the reflection alone (see `pd_gltf.py`).
        Ignoring it here would preview a gun the engine never draws.
        """
        try:
            mat = self.g.doc["materials"][prim["material"]]
            return mat["pbrMetallicRoughness"].get("baseColorFactor", [1.0, 1.0, 1.0, 1.0])
        except (KeyError, IndexError):
            return [1.0, 1.0, 1.0, 1.0]

    def _prim_image(self, prim):
        try:
            mat = self.g.doc["materials"][prim["material"]]
            tex = mat["pbrMetallicRoughness"]["baseColorTexture"]["index"]
            src = self.g.doc["textures"][tex]["source"]
            img = self.g.doc["images"][src]
            view = self.g.doc["bufferViews"][img["bufferView"]]
            off = view.get("byteOffset", 0)
            return decode_png(self.g.bin[off : off + view["byteLength"]])
        except (KeyError, IndexError, ValueError):
            return None

    # -- posing ------------------------------------------------------------

    def joint_matrices(self, locals_=None):
        locals_ = self.bind if locals_ is None else locals_
        globals_: list[list | None] = [None] * len(self.bind)

        def resolve(i):
            if globals_[i] is None:
                p = self.parents[i]
                globals_[i] = locals_[i] if p is None else m_mul(resolve(p), locals_[i])
            return globals_[i]

        return [m_mul(resolve(i), self.ibm[i]) for i in range(len(self.bind))]

    def skin_positions(self, joints):
        out = []
        for v, j, w in zip(self.verts, self.jnts, self.wgts):
            x = y = z = 0.0
            total = 0.0
            for k in range(4):
                if w[k] == 0.0:
                    continue
                px, py, pz = m_point(joints[j[k]], v)
                x += w[k] * px
                y += w[k] * py
                z += w[k] * pz
                total += w[k]
            if total == 0.0:
                x, y, z = v
            out.append((x, y, z))
        return out


class Clip:
    """An animation-only GLB, its channels bound to a model's joints by name."""

    def __init__(self, path: str, model: Model):
        g = Gltf(path)
        anim = g.doc["animations"][0]
        self.name = anim.get("name", os.path.basename(path))
        by_name = {n: i for i, n in enumerate(model.names)}
        self.channels = []
        self.duration = 0.0
        for ch in anim["channels"]:
            node = ch["target"]["node"]
            joint = by_name.get(g.doc["nodes"][node].get("name"))
            if joint is None:
                continue
            smp = anim["samplers"][ch["sampler"]]
            times = [t[0] for t in g.accessor(smp["input"])]
            values = g.accessor(smp["output"])
            interp = smp.get("interpolation", "LINEAR")
            # The GoldenEye clip GLBs store GL enums here instead of the spec's
            # strings; `engine/src/skeletal/clip.rs` patches the same thing.
            if not isinstance(interp, str):
                interp = "STEP" if interp == 9728 else "LINEAR"
            self.duration = max(self.duration, times[-1] if times else 0.0)
            self.channels.append((joint, ch["target"]["path"], times, values, interp))

    def locals(self, model: Model, t: float):
        trs = []
        for m in model.bind:
            # Decompose the bind matrix so a rotation-only channel keeps the bind
            # translation/scale — the same rule `clip.rs` follows.
            trs.append([[m[0][3], m[1][3], m[2][3]], quat_of(m), scale_of(m)])
        for joint, path, times, values, interp in self.channels:
            v = sample(times, values, interp, t)
            if path == "rotation":
                trs[joint][1] = list(v)
            elif path == "translation":
                trs[joint][0] = list(v)
            elif path == "scale":
                trs[joint][2] = list(v)
        return [m_from_trs(t_, r_, s_) for t_, r_, s_ in trs]


def sample(times, values, interp, t):
    if not times:
        return values[0]
    if t <= times[0]:
        return values[0] if interp != "CUBICSPLINE" else values[1]
    if t >= times[-1]:
        return values[-1] if interp != "CUBICSPLINE" else values[-2]
    i = max(0, sum(1 for x in times if x <= t) - 1)
    j = min(i + 1, len(times) - 1)
    span = times[j] - times[i]
    a = (t - times[i]) / span if span > 1e-9 else 0.0
    if interp == "STEP":
        return values[i]
    if interp == "CUBICSPLINE":
        return values[3 * i + 1]
    v0, v1 = values[i], values[j]
    if len(v0) == 4:
        return quat_slerp(list(v0), list(v1), a)
    return [v0[k] + (v1[k] - v0[k]) * a for k in range(len(v0))]


def quat_of(m):
    """Rotation quaternion of a TRS matrix (scale divided out)."""
    s = scale_of(m)
    r = [[m[i][j] / (s[j] or 1.0) for j in range(3)] for i in range(3)]
    tr = r[0][0] + r[1][1] + r[2][2]
    if tr > 0:
        k = math.sqrt(tr + 1.0) * 2
        return [(r[2][1] - r[1][2]) / k, (r[0][2] - r[2][0]) / k, (r[1][0] - r[0][1]) / k, 0.25 * k]
    if r[0][0] > r[1][1] and r[0][0] > r[2][2]:
        k = math.sqrt(1 + r[0][0] - r[1][1] - r[2][2]) * 2
        return [0.25 * k, (r[0][1] + r[1][0]) / k, (r[0][2] + r[2][0]) / k, (r[2][1] - r[1][2]) / k]
    if r[1][1] > r[2][2]:
        k = math.sqrt(1 + r[1][1] - r[0][0] - r[2][2]) * 2
        return [(r[0][1] + r[1][0]) / k, 0.25 * k, (r[1][2] + r[2][1]) / k, (r[0][2] - r[2][0]) / k]
    k = math.sqrt(1 + r[2][2] - r[0][0] - r[1][1]) * 2
    return [(r[0][2] + r[2][0]) / k, (r[1][2] + r[2][1]) / k, 0.25 * k, (r[1][0] - r[0][1]) / k]


def scale_of(m):
    return [math.sqrt(sum(m[i][j] ** 2 for i in range(3))) for j in range(3)]


# ---------------------------------------------------------------------------
# PNG decode (for the embedded base-colour textures)
# ---------------------------------------------------------------------------


def decode_png(data: bytes):
    """Decode an 8-bit RGB/RGBA PNG to `(width, height, rgba)`."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    off, idat, w, h, chans = 8, b"", 0, 0, 4
    while off + 8 <= len(data):
        ln = struct.unpack_from(">I", data, off)[0]
        tag = data[off + 4 : off + 8]
        body = data[off + 8 : off + 8 + ln]
        if tag == b"IHDR":
            w, h, depth, ctype = struct.unpack_from(">IIBB", body, 0)
            if depth != 8 or ctype not in (2, 6):
                raise ValueError(f"unsupported PNG depth {depth} colour type {ctype}")
            chans = 3 if ctype == 2 else 4
        elif tag == b"IDAT":
            idat += body
        elif tag == b"IEND":
            break
        off += 12 + ln
    raw = zlib.decompress(idat)
    stride = w * chans
    out = bytearray(w * h * 4)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        ft = raw[p]
        line = bytearray(raw[p + 1 : p + 1 + stride])
        p += 1 + stride
        for i in range(stride):
            a = line[i - chans] if i >= chans else 0
            b = prev[i]
            c = prev[i - chans] if i >= chans else 0
            if ft == 1:
                line[i] = (line[i] + a) & 0xFF
            elif ft == 2:
                line[i] = (line[i] + b) & 0xFF
            elif ft == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif ft == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 0xFF
        for x in range(w):
            s, d = x * chans, (y * w + x) * 4
            out[d : d + 3] = line[s : s + 3]
            out[d + 3] = line[s + 3] if chans == 4 else 255
        prev = line
    return (w, h, bytes(out))


def sample_image(img, u, v, with_alpha=False):
    if img is None:
        return (200, 200, 200, 255) if with_alpha else (200, 200, 200)
    w, h, px = img
    x = min(w - 1, max(0, int(u * w)))
    y = min(h - 1, max(0, int(v * h)))
    o = (y * w + x) * 4
    if with_alpha:
        return (px[o], px[o + 1], px[o + 2], px[o + 3])
    return (px[o], px[o + 1], px[o + 2])


# ---------------------------------------------------------------------------
# Rasteriser
# ---------------------------------------------------------------------------


def render(
    model: Model,
    positions,
    width: int,
    height: int,
    yaw: float,
    pitch: float,
    highlight: set[int] | None = None,
    bg=(24, 26, 30),
    frame_radius: float | None = None,
    viewmodel: bool = False,
):
    """Z-buffered flat-shaded render, framed on the posed model's own bounds.

    Framing per call is deliberate: an animation frame that flings a limb, or a
    body exported at the wrong scale, still fills the picture, so the *shape* is
    what is being judged rather than the framing. Pass `frame_radius` (in the
    model's own units) to pin the camera instead — that is what makes two
    different assets comparable in size rather than each filling its own frame.

    `viewmodel=True` swaps the flat two-sided lambert for a transliteration of
    `engine/src/render/shaders/shader_viewmodel.wgsl`: **unlit** `texel x vertex
    colour`, plus the matcap environment reflection sampled by the vertex normal
    and added at [`ENV_GAIN`]. That is what a first-person gun actually looks like
    in the game, and it is the only way to see the reflection decision (half the
    PD arsenal has an `EnvMapping` material) without launching anything. The
    default shading stays as it was — it reads shape better, which is what the
    character checks want.
    """
    lo = [min(p[i] for p in positions) for i in range(3)]
    hi = [max(p[i] for p in positions) for i in range(3)]
    centre = [(lo[i] + hi[i]) / 2 for i in range(3)]
    radius = max(1e-6, max(hi[i] - lo[i] for i in range(3)) / 2)
    if frame_radius:
        # Keep the model's feet where they are and grow the frame around it, so a
        # shorter body reads as shorter rather than as further away.
        centre[1] = lo[1] + frame_radius
        radius = frame_radius

    # Orbit camera: yaw about +Y, then pitch, looking at the model centre. Pulled
    # back far enough that the bounding sphere fits the *vertical* field of view
    # (the viewport is portrait), plus a margin so nothing touches the edge.
    cy, sy = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))
    cp, sp = math.cos(math.radians(pitch)), math.sin(math.radians(pitch))
    fov_v = math.radians(35.0)
    dist = radius / math.tan(fov_v / 2) * 1.25
    eye = [
        centre[0] + dist * cp * sy,
        centre[1] + dist * sp,
        centre[2] + dist * cp * cy,
    ]
    fwd = [centre[i] - eye[i] for i in range(3)]
    n = math.sqrt(sum(v * v for v in fwd))
    fwd = [v / n for v in fwd]
    right = [fwd[2] * 0 - fwd[1] * 0, 0, 0]
    # right = normalize(fwd x up), up = right x fwd
    up0 = [0.0, 1.0, 0.0]
    right = [
        fwd[1] * up0[2] - fwd[2] * up0[1],
        fwd[2] * up0[0] - fwd[0] * up0[2],
        fwd[0] * up0[1] - fwd[1] * up0[0],
    ]
    n = math.sqrt(sum(v * v for v in right)) or 1.0
    right = [v / n for v in right]
    up = [
        right[1] * fwd[2] - right[2] * fwd[1],
        right[2] * fwd[0] - right[0] * fwd[2],
        right[0] * fwd[1] - right[1] * fwd[0],
    ]

    f = 1.0 / math.tan(fov_v / 2)
    aspect = width / height

    def project(p):
        d = [p[i] - eye[i] for i in range(3)]
        vx = sum(d[i] * right[i] for i in range(3))
        vy = sum(d[i] * up[i] for i in range(3))
        vz = sum(d[i] * fwd[i] for i in range(3))
        if vz <= 1e-6:
            return None
        return ((vx / vz * f / aspect * 0.5 + 0.5) * width,
                (0.5 - vy / vz * f * 0.5) * height,
                vz)

    fb = bytearray()
    for _ in range(width * height):
        fb += bytes((bg[0], bg[1], bg[2], 255))
    zb = [float("inf")] * (width * height)

    env = model.env_image() if viewmodel else None
    # Whole-model (GoldenEye) vs only the EnvMapping primitives (PD) — see
    # `Model.env_per_material`, which mirrors the game's `env_scope`.
    env_only_tagged = model.env_per_material()

    for a, b, c, prim in model.tris:
        pa, pb, pc = positions[a], positions[b], positions[c]
        sa, sb, sc = project(pa), project(pb), project(pc)
        if sa is None or sb is None or sc is None:
            continue
        # Face normal in world space, for shading only — winding is not trusted
        # (the N64 source may wind either way), so the light is applied two-sided.
        e1 = [pb[i] - pa[i] for i in range(3)]
        e2 = [pc[i] - pa[i] for i in range(3)]
        nx = e1[1] * e2[2] - e1[2] * e2[1]
        ny = e1[2] * e2[0] - e1[0] * e2[2]
        nz = e1[0] * e2[1] - e1[1] * e2[0]
        nl = math.sqrt(nx * nx + ny * ny + nz * nz) or 1.0
        ldir = [-right[i] * 0.4 + up[i] * 0.5 - fwd[i] for i in range(3)]
        ln = math.sqrt(sum(v * v for v in ldir)) or 1.0
        lam = abs((nx * ldir[0] + ny * ldir[1] + nz * ldir[2]) / (nl * ln))
        shade = 1.0 if viewmodel else 0.35 + 0.65 * lam

        img = model.images[prim]
        fac = model.factors[prim]
        prim_env = env if (not env_only_tagged or model.env_prim[prim]) else None
        forced = (255, 60, 60) if (highlight and model.jnts[a][0] in highlight) else None
        # Perspective-correct texturing: interpolate u/z, v/z and 1/z, then divide.
        # Affine interpolation is visibly wrong on the big torso triangles at this
        # field of view, which would read as a UV bug rather than a renderer one.
        iza, izb, izc = 1.0 / sa[2], 1.0 / sb[2], 1.0 / sc[2]
        uva, uvb, uvc = model.uvs[a], model.uvs[b], model.uvs[c]
        ca, cb, cc = model.cols[a], model.cols[b], model.cols[c]
        na, nb, nc = model.nrms[a], model.nrms[b], model.nrms[c]

        minx = max(0, int(min(sa[0], sb[0], sc[0])))
        maxx = min(width - 1, int(max(sa[0], sb[0], sc[0])) + 1)
        miny = max(0, int(min(sa[1], sb[1], sc[1])))
        maxy = min(height - 1, int(max(sa[1], sb[1], sc[1])) + 1)
        area = (sb[0] - sa[0]) * (sc[1] - sa[1]) - (sb[1] - sa[1]) * (sc[0] - sa[0])
        if abs(area) < 1e-9:
            continue
        for py in range(miny, maxy + 1):
            for px in range(minx, maxx + 1):
                x, y = px + 0.5, py + 0.5
                w0 = ((sb[0] - sa[0]) * (y - sa[1]) - (sb[1] - sa[1]) * (x - sa[0])) / area
                w1 = ((x - sa[0]) * (sc[1] - sa[1]) - (y - sa[1]) * (sc[0] - sa[0])) / area
                if w0 < 0 or w1 < 0 or w0 + w1 > 1:
                    continue
                z = sa[2] + w1 * (sb[2] - sa[2]) + w0 * (sc[2] - sa[2])
                o = py * width + px
                if z >= zb[o]:
                    continue
                if forced:
                    r, g, bl = forced
                else:
                    wa = 1.0 - w0 - w1
                    iz = wa * iza + w1 * izb + w0 * izc
                    if iz <= 1e-12:
                        continue
                    u = (wa * uva[0] * iza + w1 * uvb[0] * izb + w0 * uvc[0] * izc) / iz
                    v = (wa * uva[1] * iza + w1 * uvb[1] * izb + w0 * uvc[1] * izc) / iz
                    if viewmodel:
                        # `shader_viewmodel.wgsl` discards fully transparent texels —
                        # a cut-out is a hole, not a black pixel. Same rule here or the
                        # preview shows patches the game does not.
                        r, g, bl, al = sample_image(img, u, v, with_alpha=True)
                        if al == 0:
                            continue
                    else:
                        r, g, bl = sample_image(img, u, v)
                    # texel x vertex colour, matching `shader_viewmodel.wgsl`.
                    vc = (
                        wa * ca[0] * iza + w1 * cb[0] * izb + w0 * cc[0] * izc,
                        wa * ca[1] * iza + w1 * cb[1] * izb + w0 * cc[1] * izc,
                        wa * ca[2] * iza + w1 * cb[2] * izb + w0 * cc[2] * izc,
                    )
                    r *= vc[0] / iz * fac[0]
                    g *= vc[1] / iz * fac[1]
                    bl *= vc[2] / iz * fac[2]
                    if prim_env is not None:
                        # `shader_viewmodel.wgsl`: matcap the normal's XY into the
                        # reflection map and ADD it. The normal is used untransformed
                        # there too, so this is the same lookup, not an analogue.
                        vn = [
                            (wa * na[k] * iza + w1 * nb[k] * izb + w0 * nc[k] * izc) / iz
                            for k in range(3)
                        ]
                        vl = math.sqrt(sum(t * t for t in vn)) or 1.0
                        er, eg, eb = sample_image(
                            prim_env, vn[0] / vl * 0.5 + 0.5, vn[1] / vl * 0.5 + 0.5
                        )
                        r += er * ENV_GAIN
                        g += eg * ENV_GAIN
                        bl += eb * ENV_GAIN
                zb[o] = z
                fb[o * 4 : o * 4 + 3] = bytes(
                    (
                        min(255, int(r * shade)),
                        min(255, int(g * shade)),
                        min(255, int(bl * shade)),
                    )
                )
    return bytes(fb)


def tile(images, width, height, cols):
    """Lay rendered frames out left-to-right into one contact sheet."""
    rows = (len(images) + cols - 1) // cols
    out = bytearray(width * cols * height * rows * 4)
    total_w = width * cols
    for k, img in enumerate(images):
        ox, oy = (k % cols) * width, (k // cols) * height
        for y in range(height):
            src = y * width * 4
            dst = ((oy + y) * total_w + ox) * 4
            out[dst : dst + width * 4] = img[src : src + width * 4]
    return bytes(out), total_w, height * rows


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("model")
    ap.add_argument("out")
    ap.add_argument("--clip", default=None, help="animation GLB to pose with")
    ap.add_argument("--frames", type=int, default=1, help="samples across the clip")
    ap.add_argument("--time", type=float, default=None, help="single time in seconds")
    ap.add_argument("--yaw", type=float, default=200.0)
    ap.add_argument("--pitch", type=float, default=8.0)
    ap.add_argument("--size", type=int, default=260)
    ap.add_argument("--cols", type=int, default=6)
    ap.add_argument("--highlight", default=None, help="colour the vertices of these bones red")
    ap.add_argument(
        "--frame-radius",
        type=float,
        default=None,
        help="pin the camera to this half-extent (model units) instead of auto-framing, "
        "so two assets can be compared at true relative size",
    )
    ap.add_argument(
        "--viewmodel",
        action="store_true",
        help="shade like the ENGINE's first-person gun pass (unlit texel x vertex "
        "colour + the EnvMapping matcap) instead of the shape-reading lambert",
    )
    ap.add_argument(
        "--positions",
        default=None,
        help="raw f32 [frames x verts x xyz] from `cargo run --example pd_pose_dump` — "
        "render the ENGINE's skinning instead of this script's",
    )
    args = ap.parse_args()

    model = Model(args.model)
    highlight = None
    if args.highlight:
        want = {s.strip() for s in args.highlight.split(",")}
        highlight = {i for i, n in enumerate(model.names) if n in want}
        print(f"highlighting joints {sorted(highlight)} ({args.highlight})")

    w, h = args.size, int(args.size * 1.4)
    frames = []
    if args.positions:
        # Engine-computed poses: same mesh, same camera, positions from Rust. Anything
        # the two skinning implementations disagree about shows up as a broken figure.
        raw = open(args.positions, "rb").read()
        n = len(model.verts)
        stride = n * 3 * 4
        count = len(raw) // stride
        if count == 0:
            raise SystemExit(
                f"{args.positions}: {len(raw)} bytes is not a whole frame of {n} vertices"
            )
        print(f"engine poses: {count} frame(s) x {n} verts from {args.positions}")
        for f in range(count):
            vals = struct.unpack_from(f"<{n * 3}f", raw, f * stride)
            pos = [tuple(vals[i * 3 : i * 3 + 3]) for i in range(n)]
            frames.append(render(model, pos, w, h, args.yaw, args.pitch, highlight, frame_radius=args.frame_radius, viewmodel=args.viewmodel))
    elif args.clip:
        clip = Clip(args.clip, model)
        bound = len({c[0] for c in clip.channels})
        print(
            f"clip {clip.name}: {len(clip.channels)} channels on {bound}/{len(model.names)} "
            f"joints, {clip.duration:.2f}s"
        )
        if args.time is not None:
            ts = [args.time]
        else:
            ts = [clip.duration * i / max(args.frames, 1) for i in range(args.frames)]
        for t in ts:
            joints = model.joint_matrices(clip.locals(model, t))
            frames.append(render(model, model.skin_positions(joints), w, h, args.yaw, args.pitch, highlight, frame_radius=args.frame_radius, viewmodel=args.viewmodel))
    else:
        joints = model.joint_matrices()
        frames.append(render(model, model.skin_positions(joints), w, h, args.yaw, args.pitch, highlight, frame_radius=args.frame_radius, viewmodel=args.viewmodel))

    px, tw, th = tile(frames, w, h, min(args.cols, len(frames)))
    with open(args.out, "wb") as fh:
        fh.write(png_bytes(tw, th, px))
    print(f"{args.model} -> {args.out} ({tw}x{th}, {len(frames)} frame(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
