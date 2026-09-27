#!/usr/bin/env python3
"""Perfect Dark character/animation -> glTF binary (.glb) exporter.

Turns a PD `chrs/*.bin` into a **skinned** GLB and a PD `animations/*.bin` into a
clip-only GLB, in exactly the shape the engine's existing GoldenEye character
pipeline already consumes (`engine/src/skeletal/gltf_skin.rs` for the body,
`engine/src/skeletal/clip.rs` for the clips). No new Rust loader is needed: a PD
character arrives down the same path a GE one does.

Read `START_HERE.md` section 2 for the underlying PD formats; this file only
covers what is specific to *re-expressing* them as glTF.

# The rig is renamed onto GoldenEye's bone names

The game addresses bones by name — `Bone_9` is the weapon hand, `Bone_3` the
head, `Bone_14`/`Bone_15` the feet (see `game/src/combat/enemy_weapons.rs`). PD's
15 joints are the same 15 joints, numbered differently, so this exporter renames
them onto the GE names and every existing system (weapon attach, head look-at,
foot IK, the aim overlay, the upper-body mask) works on a PD body unmodified.

Which PD part is which was **not** guessed from the +/-X sign. `modeldef.parts`
is a sorted lookup table from `MODELPART_CHR_*` to node, and it settles it
outright, identically across a51guard / dark_frock / maian_soldier / mrblonde:

    MODELPART_CHR_RIGHTHAND (3) -> anim part 8   (the -X arm chain 4->6->8)
    MODELPART_CHR_LEFTHAND  (5) -> anim part 7   (the +X arm chain 3->5->7)
    MODELPART_CHR_0006      (6) -> anim part 2   (the head, HEADSPOT's sibling)

So **-X is the character's right**, and `PART_TO_BONE` below follows from that.

# Bind pose: identity inverse-bind matrices, vertices in bone-local space

PD skins per vertex by binding a bone matrix mid-display-list (`G_MTX`) and then
transforming the raw vertex by that bone's *world* matrix — i.e. PD vertices are
already stored in **bone-local space**. glTF computes `jointMatrix = global(joint)
x inverseBind(joint)`, so writing `inverseBind = identity` and keeping vertices
bone-local makes `jointMatrix` exactly PD's bone world matrix. The conversion is
then a pure re-encoding with no change of space, which is the point: there is no
opportunity for a silent transform error.

(The GoldenEye assets use the other convention — model-space vertices, real
inverse-bind matrices, so their bind pose reduces to identity joint matrices. Both
are valid glTF; nothing in the loader assumes either.)

Consequently a PD body's `bounds_min/max` (a bone-local AABB) is meaningless.
`World::new` only falls back to it when no idle clip loaded, and the PD path
always has one.

# Blend joints are real joints here, not an approximation

A PD `POSITION` node owns up to three matrix slots. Slot 0 is the joint; slots 1
and 2 are *blend* matrices, and `lib/modelasm_c.c:367-430` builds them as the
parent matrix composed with **half** the joint's rotation (the quaternion
half-angle dance at :380-425) — a midpoint frame that hides the seam at a joint.

Rather than fold those vertices onto the owning joint (what `pd_pose.py` does,
and which leaves a visible crease), each blend slot becomes its own glTF joint,
`Blend_<bone>`, sharing the owning bone's parent and rest offset and carrying the
half-rotation per frame. That is exact, not approximate, and costs 4 extra joints
on a 15-bone rig. `--no-blend-joints` reverts to the folded behaviour for A/B.

The 15 named `Bone_*` joints keep their names and roles either way, so nothing
that looks a bone up by name notices the extra joints.

# Units

The engine draws characters at `CHAR_SCALE` (`game/src/world/mod.rs`), a single
global constant, so bodies must arrive in the same units GE bodies use rather
than in metres. `EXPORT_SCALE` converts PD millimetres to those units; the result
is that a PD body needs no per-body scale in the engine. `--metres` overrides it
for inspection in an external tool.

Usage:
    python pd_gltf.py char  <chr.bin> <out.glb>
    python pd_gltf.py clip  <ANIM_ID> <out.glb> [--fps 30]
    python pd_gltf.py batch <manifest.json> <outdir>
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pd_tex  # noqa: E402
from pd_anim import load_animation  # noqa: E402
from pd_model import VTX_SIZE, ModelDef, load, seg_off, seg_ok  # noqa: E402
from pd_pose import UNITS_PER_METRE, build_skeleton, rotation_matrix  # noqa: E402

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

# `CHAR_SCALE` in `native/crates/game/src/world/mod.rs` — the single global scale
# every skinned body is drawn at. Exporting in these units means a PD body drops
# into the roster with no per-body scale of its own.
CHAR_SCALE = 0.000_832
GE_UNITS_PER_METRE = 1.0 / CHAR_SCALE
#: PD model units (millimetres) -> engine character units.
EXPORT_SCALE = GE_UNITS_PER_METRE / UNITS_PER_METRE

#: Animation playback rate, in PD animation frames per second.
#:
#: Derived from `chr_action_go_to_position` (`game/chraction.c:2189`), which sets
#: an ETA in 60Hz frames as
#:
#:     eta60 = distance / (movedist_per_anim_frame * mult)
#:
#: For that to be dimensionally true, animation frames must advance by `mult`
#: per 60Hz frame. Locomotion passes `mult = 0.5`, so 30 animation frames per
#: second. (`model_tick_anim`'s `lvupdate240` loop is the same statement written
#: per-tick.) Every locomotion clip checked is authored in place — part 0 has no
#: translation channel at all — so there is no stride/root-motion cross-check to
#: be had; this is the derivation, and the eye is the confirmation.
DEFAULT_FPS = 30.0

#: PD animation part number -> GoldenEye bone number.
#:
#: The chain shapes are read straight off the node tree (`pd_pose.py --skeleton`):
#: root 0; spine 1 with head 2; arms 3->5->7 (+X) and 4->6->8 (-X); legs 9->11->13
#: (+X) and 10->12->14 (-X). Sidedness comes from `modeldef.parts` (see the module
#: docstring): -X is the character's right.
#:
#: The GE side is fixed by `game/src/combat/enemy_weapons.rs`: Bone_9 right hand,
#: Bone_8 left hand, Bone_3 head, Bone_1 pelvis, Bone_14 left foot, Bone_15 right
#: foot, with the leg chains 10->12->14 (left) and 11->13->15 (right).
PART_TO_BONE = {
    0: 1,    # CHRINFO root      -> pelvis
    1: 2,    # spine             -> chest
    2: 3,    # head/neck         -> head
    3: 4,   5: 6,   7: 8,    # +X arm  -> LEFT  upper arm / forearm / hand
    4: 5,   6: 7,   8: 9,    # -X arm  -> RIGHT upper arm / forearm / hand
    9: 10, 11: 12, 13: 14,   # +X leg  -> LEFT  thigh / shin / foot
    10: 11, 12: 13, 14: 15,  # -X leg  -> RIGHT thigh / shin / foot
}

#: Per-part debug palette, matching `pd_pose.py`'s so the skinned bodies read the
#: same as the preview props they replace. PD character models are *textured* —
#: `modelrodata_dl.colours` is NULL and every vertex carries real `s,t` — but the
#: textures are an inline N64 blob this pipeline does not decode yet, so parts get
#: flat colours for now. Desaturated on purpose: the silhouette should read as a
#: figure while a mis-parented limb still jumps out.
PALETTE = [
    (0.72, 0.68, 0.62), (0.45, 0.50, 0.58), (0.62, 0.48, 0.42),
    (0.50, 0.58, 0.48), (0.66, 0.60, 0.45), (0.42, 0.46, 0.56),
    (0.70, 0.55, 0.50), (0.52, 0.56, 0.62),
]
#: Side of one palette cell, in texels. Larger than 1x1 so that a linear/mipmapped
#: sampler cannot bleed one part's colour into its neighbour's.
PALETTE_CELL = 8

# glTF component types.
CT_U16 = 5123
CT_U32 = 5125
CT_F32 = 5126
# glTF bufferView targets.
TARGET_ARRAY = 34962
TARGET_ELEMENT = 34963

GLB_MAGIC = 0x46546C67
CHUNK_JSON = 0x4E4F534A
CHUNK_BIN = 0x004E4942


# ---------------------------------------------------------------------------
# Small maths helpers (row-vector PD -> column-vector glTF)
# ---------------------------------------------------------------------------


def quat_from_pd_rotation(rx: float, ry: float, rz: float) -> tuple[float, float, float, float]:
    """PD Euler XYZ -> a glTF `[x, y, z, w]` quaternion.

    `pd_pose.rotation_matrix` returns PD's row-vector matrix (`v' = v @ M`).
    glTF/glam are column-vector (`v' = M @ v`), and the two differ by exactly a
    transpose, so the rotation basis is read out of the *columns* here. Both are
    proper rotations, so the quaternion conversion is well-defined either way —
    getting the transpose backwards inverts every joint rotation, which is
    florid on screen rather than subtle.
    """
    r = rotation_matrix(rx, ry, rz)
    # m[i][j] with the transpose folded in: m_col[i][j] = r[j][i].
    m = [[r[j][i] for j in range(3)] for i in range(3)]
    return quat_from_matrix(m)


def quat_from_matrix(m) -> tuple[float, float, float, float]:
    """Column-vector 3x3 rotation matrix -> `[x, y, z, w]` (Shepperd's method)."""
    trace = m[0][0] + m[1][1] + m[2][2]
    if trace > 0.0:
        s = math.sqrt(trace + 1.0) * 2.0
        w = 0.25 * s
        x = (m[2][1] - m[1][2]) / s
        y = (m[0][2] - m[2][0]) / s
        z = (m[1][0] - m[0][1]) / s
    elif m[0][0] > m[1][1] and m[0][0] > m[2][2]:
        s = math.sqrt(1.0 + m[0][0] - m[1][1] - m[2][2]) * 2.0
        w = (m[2][1] - m[1][2]) / s
        x = 0.25 * s
        y = (m[0][1] + m[1][0]) / s
        z = (m[0][2] + m[2][0]) / s
    elif m[1][1] > m[2][2]:
        s = math.sqrt(1.0 + m[1][1] - m[0][0] - m[2][2]) * 2.0
        w = (m[0][2] - m[2][0]) / s
        x = (m[0][1] + m[1][0]) / s
        y = 0.25 * s
        z = (m[1][2] + m[2][1]) / s
    else:
        s = math.sqrt(1.0 + m[2][2] - m[0][0] - m[1][1]) * 2.0
        w = (m[1][0] - m[0][1]) / s
        x = (m[0][2] + m[2][0]) / s
        y = (m[1][2] + m[2][1]) / s
        z = 0.25 * s
    n = math.sqrt(x * x + y * y + z * z + w * w) or 1.0
    return (x / n, y / n, z / n, w / n)


def quat_halfway(q) -> tuple[float, float, float, float]:
    """Half of a rotation: `slerp(identity, q, 0.5)`, normalised.

    This is what `lib/modelasm_c.c` computes for a `POSITION` node's blend
    matrix. Taking the shorter arc (negating `q` when `w < 0`) matters — the
    other arc halves to a rotation the long way round, which reads as a limb
    snapping through the body.
    """
    x, y, z, w = q
    if w < 0.0:
        x, y, z, w = -x, -y, -z, -w
    w += 1.0  # q + identity; renormalising it is the midpoint of the arc
    n = math.sqrt(x * x + y * y + z * z + w * w)
    if n < 1e-9:
        return (0.0, 0.0, 0.0, 1.0)
    return (x / n, y / n, z / n, w / n)


# ---------------------------------------------------------------------------
# PNG (palette textures, and reused by pd_preview.py for its output)
# ---------------------------------------------------------------------------


def png_bytes(width: int, height: int, rgba: bytes) -> bytes:
    """Encode tightly-packed RGBA8 as a PNG (no filtering, one IDAT)."""
    raw = bytearray()
    stride = width * 4
    for y in range(height):
        raw.append(0)  # filter type 0 (None)
        raw += rgba[y * stride : (y + 1) * stride]

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def palette_png(count: int) -> bytes:
    """A horizontal strip of `count` solid `PALETTE` cells, `PALETTE_CELL` square."""
    w, h = max(count, 1) * PALETTE_CELL, PALETTE_CELL
    row = bytearray()
    for i in range(max(count, 1)):
        r, g, b = PALETTE[i % len(PALETTE)]
        px = bytes((int(r * 255), int(g * 255), int(b * 255), 255))
        row += px * PALETTE_CELL
    return png_bytes(w, h, bytes(row) * h)


def palette_uv(index: int, count: int) -> tuple[float, float]:
    """The centre of palette cell `index` in a `count`-cell strip."""
    return ((index + 0.5) / max(count, 1), 0.5)


# ---------------------------------------------------------------------------
# Textures
# ---------------------------------------------------------------------------

#: `struct textureconfig` (`types.h`) is 12 bytes:
#: `void *texturenum; u8 width, height, level, s, t, x, y, unk0b`.
TEXCONFIG_SIZE = 12
#: Texel size in bytes. Every `G_SETTIMG` in the character display lists declares
#: `fmt=RGBA siz=16b` — checked across the models, with no other combination
#: appearing — so the inline data is RGBA5551.
TEXEL_BYTES = 2


class TexConfig:
    """One entry of `modeldef.texconfigs`: where a texture is and how big."""

    __slots__ = ("index", "ptr", "width", "height", "levels", "wrap_s", "wrap_t", "texnum")

    def __init__(self, index, ptr, width, height, levels, wrap_s, wrap_t, texnum=None):
        self.index = index
        self.ptr = ptr
        self.width = width
        self.height = height
        #: Number of mip levels present. Only level 0 is exported; the engine
        #: builds its own. Confirmed against the data: a 32x48 with `levels == 4`
        #: occupies exactly `(32*48 + 16*24 + 8*12 + 4*6) * 2` bytes.
        self.levels = levels
        self.wrap_s = wrap_s
        self.wrap_t = wrap_t
        #: Global-pool texture number, or `None` when the data is inline.
        self.texnum = texnum

    @property
    def inline(self) -> bool:
        return self.texnum is None


def read_texconfigs(m: ModelDef) -> dict[int, TexConfig]:
    """`modeldef.texconfigs`, keyed by the value the display lists' `G_SETTIMG`
    carries — which is what makes the triangle-to-texture binding exact rather
    than positional.

    `texturenum` is **one of two things**, and the segment nibble says which:

    * a **segment-05 pointer** into the model file, where the data is
      already-decompressed `RGBA5551` (see [`decode_texture`]). Only a51guard,
      dd_shock, elvis and testchr are like this;
    * a plain **index into the global texture pool** (`textures/`, 3,503 files),
      which is compressed — decoded by [`pd_tex`]. Every other body, and all 76
      head models.
    """
    out: dict[int, TexConfig] = {}
    if not (m.numtexconfigs and seg_ok(m.texconfigs)):
        return out
    base = seg_off(m.texconfigs)
    for i in range(m.numtexconfigs):
        off = base + i * TEXCONFIG_SIZE
        if off + TEXCONFIG_SIZE > len(m.data):
            break
        ptr = struct.unpack_from(">I", m.data, off)[0]
        w, h, levels, s, t = struct.unpack_from(">BBBBB", m.data, off + 4)
        if w == 0 or h == 0:
            continue
        if seg_ok(ptr):
            out[ptr] = TexConfig(i, ptr, w, h, levels, s, t)
        elif ptr >> 24 == 0:
            out[ptr] = TexConfig(i, ptr, w, h, levels, s, t, texnum=ptr & 0xFFFFFF)
    return out


def rgba16_row_bytes(width: int) -> int:
    """Bytes per stored row of an `RGBA16` texture — `tex_swizzle`'s `wordsperrow`.

    `texdecompress.c:1927` computes `((width + 3) & 0xffc) >> 1` u32 words for
    `TEXFORMAT_RGBA16`, i.e. rows are padded out to 8 bytes. Reading rows back to
    back at `width * 2` instead shears every non-multiple-of-4 texture — a 38-wide
    one slips a texel per row, which looks like noise rather than like an
    off-by-one.
    """
    return (((width + 3) & 0xFFC) >> 1) * 4


def decode_texture(m: ModelDef, cfg: TexConfig) -> tuple[int, int, bytes]:
    """Level 0 of a texture as `(width, height, RGBA8)`, from wherever it lives.

    Pool textures go through [`pd_tex.decode`]; the rest is the inline path
    below. Raises [`pd_tex.UnsupportedTexture`] when a pool texture uses the
    codec that is not ported, which the caller turns into a palette fallback.

    Note the two storage layouts are **opposite** on swizzling: pool data
    inflates linear (PD swizzles it afterwards, on its way to the RDP), while
    inline data is already swizzled and has to be undone.
    """
    if not cfg.inline:
        t = pd_tex.load(cfg.texnum)
        if (t.width, t.height) != (cfg.width, cfg.height):
            print(
                f"  NOTE: texture {cfg.texnum:#x} is {t.width}x{t.height} but "
                f"{m.name}'s texconfig says {cfg.width}x{cfg.height}",
                file=sys.stderr,
            )
        return t.width, t.height, t.rgba
    return cfg.width, cfg.height, decode_inline_texture(m, cfg)


#: The Perfect Gold / GoldenEye Setup Editor export of the arsenal, checked into
#: the repo. See [`editor_textures`].
EDITOR_DUMP = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "..", "pd dump", "weapons"
)

#: Built once by [`editor_textures`].
_EDITOR_TEXTURES: dict[int, dict] | None = None


def editor_textures() -> dict[int, dict]:
    """`texture number -> {"bmp": path, "hints": str}` over the WHOLE editor dump.

    PD's second texture codec (`tex_inflate_non_zlib`, `texdecompress.c:699`) is
    not ported, so [`decode_texture`] raises on the textures that use it — and
    those are not a rounding error: **every one of the 76 muzzle-flash textures**
    is on that codec, plus 36 visible surfaces. An undecodable texture falls back
    to the per-part debug palette, which is why the flash read as an opaque white
    square and why the MagSec 4 (89.6% of its triangles) read as flat colour.

    The user's Perfect Gold export supplies them: 355 BMPs across 31 gun folders,
    32-bit with real alpha. This index is **global rather than per-gun** on
    purpose — `tempImgEdNNNN.bmp` is NNNN = the *global pool* texture number, the
    same number `modeldef.texconfigs` carries, so a BMP exported under the Falcon
    covers the same texture wherever else it appears (which is what extends the
    coverage to third-person `chr*` models the dump never exported). Verified
    across the dump: 214 distinct numbers, and where two folders both export one,
    the files are byte-identical — so there is no ambiguity to resolve.

    `hints` is the render intent, which the editor writes into the **material
    name** (`m4CullBothEnvMappingTexScaleS0.03125…`) in the exact convention our
    engine already parses — `textured_model.rs` looks for `*EnvMapping*`,
    `load_flash` filters on `CullBoth` — because our GoldenEye weapon GLBs came
    out of this same tool. It is a per-*material* property being folded onto a
    per-*texture* key (our exports batch by texture, as PD draws them), so where a
    texture is used both ways the **modal** hint wins; 57 of 214 are like that,
    and the counts are lopsided enough for a majority to be meaningful (0x386 is
    EnvMapping on 80 materials and plain on 24).
    """
    global _EDITOR_TEXTURES
    if _EDITOR_TEXTURES is not None:
        return _EDITOR_TEXTURES
    out: dict[int, dict] = {}
    votes: dict[int, dict[str, int]] = {}
    if os.path.isdir(EDITOR_DUMP):
        for folder in sorted(os.listdir(EDITOR_DUMP)):
            path = os.path.join(EDITOR_DUMP, folder)
            if not os.path.isdir(path):
                continue
            for f in sorted(os.listdir(path)):
                if f.startswith("tempImgEd") and f.lower().endswith(".bmp"):
                    try:
                        num = int(f[len("tempImgEd") : -4], 16)
                    except ValueError:
                        continue
                    out.setdefault(num, {"bmp": os.path.join(path, f), "hints": ""})
                elif f.lower().endswith(".mtl"):
                    cur = ""
                    for line in open(os.path.join(path, f), encoding="utf-8", errors="replace"):
                        line = line.strip()
                        if line.startswith("newmtl "):
                            # `m<N><hints>` — the leading index is the editor's own
                            # material counter and carries no intent.
                            cur = re.sub(r"^m\d+", "", line[len("newmtl ") :])
                        elif line.startswith("map_Kd ") and cur is not None:
                            tex = line[len("map_Kd ") :].strip()
                            if not tex.startswith("tempImgEd"):
                                continue
                            try:
                                num = int(tex[len("tempImgEd") : -4], 16)
                            except ValueError:
                                continue
                            votes.setdefault(num, {})[cur] = votes.setdefault(num, {}).get(cur, 0) + 1
    for num, tally in votes.items():
        if num in out:
            out[num]["hints"] = max(tally.items(), key=lambda kv: kv[1])[0]
    _EDITOR_TEXTURES = out
    return out


def read_bmp(path: str) -> tuple[int, int, bytes]:
    """A Windows BMP as `(width, height, RGBA8)`, top-down.

    Only what the editor writes: an uncompressed `BITMAPINFOHEADER` at 32 bpp
    (BGRA, and the alpha is real — the flame frames ramp 0..255 across their soft
    edge) or 24 bpp.

    **Rows are taken in file order, not bottom-up.** A positive `height` means
    bottom-up per the BMP spec, and these files all declare one — but the editor
    writes the rows already flipped, so honouring the spec turns every texture
    upside down. That is measured, not assumed: 427 of these textures are ones our
    own decoder can also produce, and **423 of them agree with PD's decode only
    when the rows are read in file order** (the other 4 are vertically symmetric,
    so they agree either way). A wrong choice here is invisible as an error — it
    just makes every gun skin subtly wrong — which is why the two pipelines are
    cross-checked against each other rather than either being trusted.
    """
    with open(path, "rb") as fh:
        data = fh.read()
    if data[:2] != b"BM":
        raise ValueError(f"{path}: not a BMP")
    pixoff = struct.unpack_from("<I", data, 10)[0]
    hdr_size, width, height, _planes, bpp, compression = struct.unpack_from("<IiiHHI", data, 14)
    if hdr_size < 40 or compression != 0 or bpp not in (24, 32):
        raise ValueError(f"{path}: unsupported BMP ({bpp}bpp, compression {compression})")
    height = abs(height)
    stride = ((width * bpp // 8) + 3) & ~3
    out = bytearray(width * height * 4)
    for y in range(height):
        src = pixoff + y * stride
        for x in range(width):
            o = src + x * (bpp // 8)
            d = (y * width + x) * 4
            out[d] = data[o + 2]
            out[d + 1] = data[o + 1]
            out[d + 2] = data[o]
            out[d + 3] = data[o + 3] if bpp == 32 else 255
    return width, height, bytes(out)


def resolve_texture(m: ModelDef, cfg: TexConfig, texnum: int) -> tuple[int, int, bytes, str]:
    """Level 0 of a texture, from PD's own data or — when our codec cannot — from
    the editor dump. Returns `(width, height, RGBA8, source)`.

    Raises [`pd_tex.UnsupportedTexture`] only when BOTH fail, which is what the
    caller turns into the debug palette.
    """
    try:
        w, h, rgba = decode_texture(m, cfg)
        return w, h, rgba, "pd"
    except pd_tex.UnsupportedTexture:
        entry = editor_textures().get(texnum)
        if entry is None:
            raise
        w, h, rgba = read_bmp(entry["bmp"])
        if (w, h) != (cfg.width, cfg.height):
            # Callers normalise UVs by the returned size for exactly this case
            # (0x606 is 4x16 in the texconfig and 8x16 here), but it is worth
            # saying out loud — a silent rescale is invisible until it is on screen.
            print(
                f"  NOTE: editor BMP for texture {texnum:#x} is {w}x{h} but "
                f"{m.name}'s texconfig says {cfg.width}x{cfg.height}",
                file=sys.stderr,
            )
        return w, h, rgba, "editor"


def decode_inline_texture(m: ModelDef, cfg: TexConfig) -> bytes:
    """Level 0 of an inline RGBA5551 texture, as tightly-packed RGBA8.

    Two storage details, both taken from `tex_swizzle` (`texdecompress.c:1927`)
    and both independently confirmed on screen before that function was found:

    * rows are padded to 8 bytes ([`rgba16_row_bytes`]);
    * **odd rows have adjacent u32 words swapped** — the N64 TMEM interleave. At
      16bpp a u32 is two texels, so the swap is `x ^ 2` within each group of four.
      Skipping it leaves a fine vertical comb over everything; the figure still
      reads, the faces do not.

    The mip levels that follow level 0 are ignored: PD generates them at load
    (`tex_shrink_*`), and so does the engine.

    `RGBA16` is `rrrrrgggggbbbbba` big-endian. The 5-bit channels are scaled by
    `*255//31` rather than `<<3` so full scale stays full scale (`31 -> 255`, not
    `248`), which otherwise greys down a whole character. The single alpha bit
    becomes 0 or 255.
    """
    base = seg_off(cfg.ptr)
    stride = rgba16_row_bytes(cfg.width)
    if base + stride * cfg.height > len(m.data):
        raise SystemExit(f"{m.name}: texture at {cfg.ptr:#x} runs past the end of the file")
    px = bytearray(cfg.width * cfg.height * 4)
    for y in range(cfg.height):
        row = base + y * stride
        swap = y & 1
        for x in range(cfg.width):
            o = row + ((x ^ 2) if swap else x) * TEXEL_BYTES
            v = (m.data[o] << 8) | m.data[o + 1]
            d = (y * cfg.width + x) * 4
            px[d] = ((v >> 11) & 31) * 255 // 31
            px[d + 1] = ((v >> 6) & 31) * 255 // 31
            px[d + 2] = ((v >> 1) & 31) * 255 // 31
            px[d + 3] = 255 if v & 1 else 0
    return bytes(px)


def mip_chain_bytes(cfg: TexConfig) -> int:
    """Total stored size of a texture including its mip levels.

    Only used as a self-check: for every inline texture but the last, this must
    equal the gap to the next one. It does, to the byte, across all 30 of
    a51guard's — which is what pins down the padded row stride independently of
    how the pixels happen to look.
    """
    w, h, total = cfg.width, cfg.height, 0
    for _ in range(max(cfg.levels, 1)):
        total += rgba16_row_bytes(w) * h
        w, h = max(w // 2, 1), max(h // 2, 1)
    return total


# ---------------------------------------------------------------------------
# GLB assembly
# ---------------------------------------------------------------------------


class Glb:
    """Minimal glTF 2.0 binary writer: one buffer, everything in the BIN chunk."""

    def __init__(self, generator: str):
        self.doc: dict = {
            "asset": {"version": "2.0", "generator": generator},
            "scene": 0,
            "scenes": [{"nodes": []}],
            "nodes": [],
        }
        self.bin = bytearray()

    # -- buffer plumbing ---------------------------------------------------

    def _view(self, data: bytes, target: int | None = None, stride: int | None = None) -> int:
        while len(self.bin) % 4:
            self.bin.append(0)
        view = {"buffer": 0, "byteOffset": len(self.bin), "byteLength": len(data)}
        if target is not None:
            view["target"] = target
        if stride is not None:
            view["byteStride"] = stride
        self.bin += data
        return self._append("bufferViews", view)

    def _append(self, key: str, value) -> int:
        arr = self.doc.setdefault(key, [])
        arr.append(value)
        return len(arr) - 1

    def accessor(
        self,
        values,
        comptype: int,
        kind: str,
        target: int | None = None,
        minmax: bool = False,
    ) -> int:
        """Pack a flat list of scalars into a bufferView + accessor.

        `values` is flat (a VEC3 accessor takes `3 * count` numbers). `minmax`
        writes the per-component min/max the spec demands for `POSITION`.
        """
        ncomp = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}[kind]
        fmt = {CT_U16: "<H", CT_U32: "<I", CT_F32: "<f"}[comptype]
        data = b"".join(struct.pack(fmt, v) for v in values)
        count = len(values) // ncomp
        acc = {
            "bufferView": self._view(data, target),
            "componentType": comptype,
            "count": count,
            "type": kind,
        }
        if minmax and count:
            lo = [min(values[i::ncomp]) for i in range(ncomp)]
            hi = [max(values[i::ncomp]) for i in range(ncomp)]
            acc["min"], acc["max"] = lo, hi
        return self._append("accessors", acc)

    def image_png(self, png: bytes) -> int:
        view = self._view(png)
        return self._append("images", {"bufferView": view, "mimeType": "image/png"})

    # -- output ------------------------------------------------------------

    def save(self, path: str) -> None:
        self.doc["buffers"] = [{"byteLength": len(self.bin)}]
        js = json.dumps(self.doc, separators=(",", ":")).encode("utf-8")
        js += b" " * (-len(js) % 4)
        bn = bytes(self.bin) + b"\0" * (-len(self.bin) % 4)
        total = 12 + 8 + len(js) + 8 + len(bn)
        with open(path, "wb") as fh:
            fh.write(struct.pack("<III", GLB_MAGIC, 2, total))
            fh.write(struct.pack("<II", len(js), CHUNK_JSON))
            fh.write(js)
            fh.write(struct.pack("<II", len(bn), CHUNK_BIN))
            fh.write(bn)


# ---------------------------------------------------------------------------
# The rig, as glTF joints
# ---------------------------------------------------------------------------


class RigJoint:
    """One glTF joint: a name, a rest offset, a parent, and where it came from."""

    __slots__ = ("name", "bone", "part", "offset", "parent", "is_blend", "owner")

    def __init__(self, name, bone, part, offset, parent, is_blend=False, owner=None):
        self.name = name
        self.bone = bone            # GE bone number (blends borrow their owner's)
        self.part = part            # PD animation part number
        self.offset = offset        # rest translation, PD units
        self.parent = parent        # index into the rig list, or None
        self.is_blend = is_blend
        self.owner = owner          # rig index of the joint this blend softens


class Rig:
    """The 15 GE-named bones (+ optional blend joints) of one PD character."""

    def __init__(self, model: ModelDef, blend_joints: bool = True):
        joints = build_skeleton(model)
        by_part = {}
        for off, j in joints.items():
            if j.part in by_part:
                raise SystemExit(f"{model.name}: duplicate animation part {j.part}")
            by_part[j.part] = j
        missing = sorted(set(PART_TO_BONE) - set(by_part))
        if missing:
            raise SystemExit(
                f"{model.name}: not the standard 15-bone chr rig — missing parts {missing}"
            )
        extra = sorted(set(by_part) - set(PART_TO_BONE))
        if extra:
            raise SystemExit(f"{model.name}: unexpected extra joints, parts {extra}")

        # Bones first, in Bone_1..Bone_15 order, so joint index == bone number - 1
        # (which is also what `clip.rs`'s index fallback assumes).
        order = sorted(PART_TO_BONE, key=lambda p: PART_TO_BONE[p])
        self.joints: list[RigJoint] = []
        row_of_part: dict[int, int] = {}
        for part in order:
            j = by_part[part]
            row_of_part[part] = len(self.joints)
            self.joints.append(
                RigJoint(f"Bone_{PART_TO_BONE[part]}", PART_TO_BONE[part], part, j.pos, None)
            )
        for part in order:
            j = by_part[part]
            parent_part = joints[j.parent].part if j.parent is not None else None
            if parent_part is not None:
                self.joints[row_of_part[part]].parent = row_of_part[parent_part]

        # A blend joint for EVERY bone, whether this character uses it or not.
        #
        # Which bones carry a blend slot varies per character — 51 of the 65 put
        # them on elbows + knees, but `elvis` uniquely uses shoulders + hips, and
        # six other layouts exist. Clips bind to joints **by name**, and one clip
        # export has to drive every body, so the name set cannot depend on the
        # body: a body with a `Blend_10` the clip has never heard of keeps its bind
        # rotation (that geometry then stays splayed in the rest pose — a flat fin
        # at the hip), and worse, the clip's unmatched `Blend_6` lands on some
        # arbitrary joint through `clip.rs`'s node-index fallback.
        #
        # So all 15 exist on every rig and in every clip. Unused ones carry no
        # vertices and cost a matrix.
        blend_row: dict[int, int] = {}
        for part in order:
            owner = self.joints[row_of_part[part]]
            blend_row[part] = len(self.joints)
            self.joints.append(
                RigJoint(
                    f"Blend_{owner.bone}",
                    owner.bone,
                    owner.part,
                    owner.offset,
                    owner.parent,
                    is_blend=True,
                    owner=row_of_part[part],
                )
            )

        # `matrix slot -> rig joint`. Slot 0 of each joint is the joint itself;
        # slot 1 is its blend frame. `mtxindexes[2]` is never used by any shipped
        # character (checked across all 65) — loud rather than silent if that
        # changes, because a third frame would need its own joint.
        self.slot_to_joint: dict[int, int] = {}
        for part in order:
            j = by_part[part]
            row = row_of_part[part]
            mi0, mi1, mi2 = j.slots
            if mi0 >= 0:
                self.slot_to_joint[mi0] = row
            if mi2 >= 0:
                raise SystemExit(
                    f"{model.name}: part {part} uses mtxindexes[2] ({mi2}), which no "
                    "shipped character does — it needs its own blend joint"
                )
            if mi1 >= 0:
                self.slot_to_joint[mi1] = blend_row[part] if blend_joints else row

    def __len__(self) -> int:
        return len(self.joints)

    def bone_index(self, bone: int) -> int:
        return next(i for i, j in enumerate(self.joints) if j.bone == bone and not j.is_blend)


def rig_nodes(glb: Glb, rig: Rig, scale: float) -> list[int]:
    """Emit `Armature` + one node per rig joint. Returns node index per joint."""
    armature = glb._append("nodes", {"name": "Armature", "children": []})
    glb.doc["scenes"][0]["nodes"].append(armature)
    node_of = [
        glb._append(
            "nodes",
            {
                "name": j.name,
                "translation": [j.offset[0] * scale, j.offset[1] * scale, j.offset[2] * scale],
            },
        )
        for j in rig.joints
    ]
    for i, j in enumerate(rig.joints):
        parent = glb.doc["nodes"][armature if j.parent is None else node_of[j.parent]]
        parent.setdefault("children", []).append(node_of[i])
    return node_of


# ---------------------------------------------------------------------------
# char: skinned character GLB
# ---------------------------------------------------------------------------


HEADSPOT_NODE = 0x17
#: The animation part number of the head joint, and the matrix slot a grafted head
#: model's display lists bind to. See `attach_head`.
HEAD_PART = 2
HEAD_MATRIX_SLOT = 0


def attach_head(body: ModelDef, rig: Rig, head_path: str):
    """Geometry for a separate `head*.bin`, bound to the body's head joint.

    62 of the 65 characters on the shared rig carry a `HEADSPOT` node (type 0x17)
    instead of a built-in head; only a51guard, dd_shock and elvis are
    self-contained. `model_attach_head` (`lib/model.c:4275`) simply reparents the
    head model's root under that node — no transform of its own; `HEADSPOT`'s
    rodata is a lone `rwdataindex`.

    So a head's vertices are already expressed in head-joint space, and its
    display lists bind matrix slot 0 (every head model has `nummatrices == 1`).
    After the graft, segment 3 is the *body's* matrix array, and slot 0 of that is
    the head joint — checked here rather than assumed, because a body that broke
    the convention would silently hang its head off the pelvis.
    """
    head = load(head_path)
    groups = head.geometry()
    if not groups:
        raise SystemExit(f"{head.name}: no geometry")
    joint = rig.slot_to_joint.get(HEAD_MATRIX_SLOT)
    if joint is None or rig.joints[joint].part != HEAD_PART:
        got = rig.joints[joint].name if joint is not None else "nothing"
        raise SystemExit(
            f"{body.name}: matrix slot {HEAD_MATRIX_SLOT} is {got}, not the head joint "
            f"(part {HEAD_PART}) — a head grafted here would land on the wrong bone"
        )
    used = {v.mtx for g in groups for v in g.verts}
    if used - {HEAD_MATRIX_SLOT, -1}:
        raise SystemExit(f"{head.name}: binds matrix slots {sorted(used)}, expected only 0")
    return head, groups, joint


def export_char(
    path: str,
    out: str,
    scale: float,
    blend_joints: bool = True,
    head_path: str | None = None,
) -> dict:
    model = load(path)
    rig = Rig(model, blend_joints=blend_joints)
    # Each entry is (source model, mesh group, forced joint or None). The source
    # model matters because a grafted head brings its own texture table.
    groups = [(model, g, None) for g in model.geometry()]
    if not groups:
        raise SystemExit(f"{model.name}: no geometry")

    has_headspot = any((n.type & 0xFF) == HEADSPOT_NODE for n in model.walk())
    head_name = None
    if head_path:
        head, head_groups, head_joint = attach_head(model, rig, head_path)
        head_name = head.name
        groups += [(head, g, head_joint) for g in head_groups]
        if not has_headspot:
            print(
                f"  NOTE: {model.name} has its own head; {head.name} is grafted on top of it",
                file=sys.stderr,
            )
    elif has_headspot:
        print(
            f"  WARNING: {model.name} has a HEADSPOT node and no --head was given — "
            "it will export headless",
            file=sys.stderr,
        )

    glb = Glb("pd_gltf.py (Perfect Dark -> glTF)")
    node_of = rig_nodes(glb, rig, scale)

    # ── Geometry, batched by texture: one glTF primitive per PD texture, which is
    # also how PD draws it (a `G_SETTIMG` binds a texture, the triangles after it
    # use it). That keeps every texture at its own size and its own wrap mode, with
    # no atlas to pack and no UVs to rescale — and the renderer already uploads one
    # image and one bind group per primitive.
    #
    # Vertices are emitted per (source vertex, texture) rather than shared, because
    # a vertex on a texture seam needs a different normalized UV for each texture it
    # borders. Vertices stay in PD bone-local space (see the module docstring) and
    # are skinned rigidly — PD binds exactly one matrix per vertex via `G_MTX`, so
    # there is nothing to weight.
    texconfigs = {id(m): read_texconfigs(m) for m in {id(m): m for m, _, _ in groups}.values()}
    # batch key -> (source model, TexConfig or None); dict keeps first-seen order.
    batches: dict[tuple, list] = {}
    unbound = 0
    untextured = 0
    # A texture the pool codec cannot decode yet drops that batch to the palette,
    # rather than failing the whole character.
    undecodable: set[tuple[int, int]] = set()
    for gi, (src, g, forced_joint) in enumerate(groups):
        cfgs = texconfigs[id(src)]
        for a, b, c, tex in g.tris:
            cfg = cfgs.get(tex)
            if cfg is not None and (id(src), tex) not in undecodable:
                try:
                    decode_texture(src, cfg)
                except pd_tex.UnsupportedTexture as e:
                    print(f"  NOTE: {src.name} texture {cfg.index}: {e}", file=sys.stderr)
                    undecodable.add((id(src), tex))
            if cfg is not None and (id(src), tex) in undecodable:
                cfg = None
            if cfg is None:
                untextured += 1
            key = (id(src), tex if cfg is not None else None)
            batch = batches.get(key)
            if batch is None:
                batch = batches[key] = [src, cfg, gi, {}, [], [], [], [], []]
            _, _, _, remap, pos, uv, jnt, wgt, idx = batch
            for corner in (a, b, c):
                local = (gi, corner)
                out_i = remap.get(local)
                if out_i is None:
                    vert = g.verts[corner]
                    out_i = remap[local] = len(pos) // 3
                    pos.extend((vert.x * scale, vert.y * scale, vert.z * scale))
                    if cfg is not None:
                        # PD texcoords are S10.5 fixed point in texels; normalize by
                        # this texture's own size.
                        uv.extend((vert.s / 32.0 / cfg.width, vert.t / 32.0 / cfg.height))
                    else:
                        uv.extend(palette_uv(gi, len(groups)))
                    # A grafted head's vertices all ride the head joint
                    # (`attach_head`), including the handful loaded before the head
                    # DL's first `G_MTX`.
                    joint = (
                        forced_joint
                        if forced_joint is not None
                        else rig.slot_to_joint.get(vert.mtx)
                    )
                    if joint is None:
                        # Would mean a `G_MTX` slot no `POSITION` node claims. None of
                        # the 65 shared-rig characters has one; loud rather than silent
                        # if that ever changes, because the vertex would otherwise sit
                        # at the origin.
                        unbound += 1
                        joint = 0
                    jnt.extend((joint, 0, 0, 0))
                    wgt.extend((1.0, 0.0, 0.0, 0.0))
                idx.append(out_i)
    if unbound:
        print(f"  WARNING: {unbound} vertices had no matrix binding", file=sys.stderr)
    if untextured:
        print(
            f"  WARNING: {untextured} triangles had no texture — they fall back to the "
            "per-part debug palette",
            file=sys.stderr,
        )

    sampler = glb._append("samplers", {"magFilter": 9728, "minFilter": 9728})  # NEAREST
    palette_image = None
    primitives = []
    nverts = 0
    ntris = 0
    for src, cfg, gi, _remap, pos, uv, jnt, wgt, idx in batches.values():
        if cfg is not None:
            tw, th, rgba = decode_texture(src, cfg)
            image = glb.image_png(png_bytes(tw, th, rgba))
            mat_name = f"tex{cfg.index:02d}_{tw}x{th}"
        else:
            if palette_image is None:
                palette_image = glb.image_png(palette_png(len(groups)))
            image = palette_image
            mat_name = f"part{gi:02d}_untextured"
        texture = glb._append("textures", {"sampler": sampler, "source": image})
        material = glb._append(
            "materials",
            {
                "name": mat_name,
                # The GE bodies carry no NORMALs either, so the renderer draws base
                # colour straight through — this is an N64 look, not a lit PBR one.
                "pbrMetallicRoughness": {
                    "baseColorTexture": {"index": texture},
                    "metallicFactor": 0.0,
                    "roughnessFactor": 1.0,
                },
                # RGBA5551's single alpha bit is all-or-nothing, which is exactly
                # what MASK means; PD draws these with alpha compare, not blending.
                "alphaMode": "MASK",
                "alphaCutoff": 0.5,
                "doubleSided": True,
            },
        )
        n = len(pos) // 3
        primitives.append(
            {
                "attributes": {
                    "POSITION": glb.accessor(pos, CT_F32, "VEC3", TARGET_ARRAY, minmax=True),
                    "TEXCOORD_0": glb.accessor(uv, CT_F32, "VEC2", TARGET_ARRAY),
                    "JOINTS_0": glb.accessor(jnt, CT_U16, "VEC4", TARGET_ARRAY),
                    "WEIGHTS_0": glb.accessor(wgt, CT_F32, "VEC4", TARGET_ARRAY),
                },
                "indices": glb.accessor(
                    idx, CT_U16 if n <= 0xFFFF else CT_U32, "SCALAR", TARGET_ELEMENT
                ),
                "material": material,
            }
        )
        nverts += n
        ntris += len(idx) // 3

    mesh = glb._append("meshes", {"name": model.name, "primitives": primitives})

    # ── Skin. Identity inverse-bind matrices: PD vertices are already bone-local.
    ident = [1.0 if i % 5 == 0 else 0.0 for i in range(16)]
    a_ibm = glb.accessor(ident * len(rig), CT_F32, "MAT4")
    skin = glb._append(
        "skins", {"name": "pd_rig", "inverseBindMatrices": a_ibm, "joints": node_of}
    )
    mesh_node = glb._append("nodes", {"name": "body", "mesh": mesh, "skin": skin})
    glb.doc["scenes"][0]["nodes"].append(mesh_node)

    glb.save(out)
    stats = {
        "model": model.name,
        "head": head_name,
        "joints": len(rig),
        "blends": sum(1 for j in rig.joints if j.is_blend),
        "verts": nverts,
        "tris": ntris,
        "parts": len(groups),
        "textures": sum(1 for _, cfg, *_ in batches.values() if cfg is not None),
    }
    print(
        f"{model.name}{' + ' + head_name if head_name else ''} -> {out}: "
        f"{stats['verts']} verts, {stats['tris']} tris, {stats['textures']} textures, "
        f"{stats['joints']} joints ({stats['blends']} blend)"
    )
    return stats


# ---------------------------------------------------------------------------
# gun: static weapon GLB + its authored attach/muzzle metadata
# ---------------------------------------------------------------------------

NODE_POSITION_T = 0x02
NODE_CHRGUNFIRE = 0x0C
NODE_POSITIONHELD = 0x15
NODE_STARGUNFIRE = 0x16

#: Named `POSITION` parts on a FIRST-PERSON gun model (`constants.h:2418-2424`).
MODELPART_GUN_MUZZLEPOS = 0x0032
MODELPART_GUN_HOLDPOS = 0x0037

#: The two parts `chr_get_gun_pos` looks for on a THIRD-PERSON gun, in order
#: (`constants.h:2584`). 0x0 is the CHRGUNFIRE node; 0x1 is POSITIONHELD.
MODELPART_0000 = 0x0000
MODELPART_0001 = 0x0001

#: The first-person muzzle-flash part. PD hides it in the weapon's authored
#: `modelpartvisibility` and shows it only while firing, so it is exported to its
#: own GLB rather than baked into the gun (where it reads as a permanent square).
MODELPART_GUN_MUZZLEFLASH1 = 0x005A

#: PD gun models are on the same 1000-units-per-metre scale as characters. NOT
#: assumed from the character pipeline — measured off the third-person meshes,
#: which come out anatomically right at that scale and wrong at any other:
#: chrfalcon2 0.213 m, chrar34 0.616 m, chrsniperrifle 0.984 m. (`modeldef.scale`
#: disagrees — 127 for chrfalcon2, 939 for guns/falcon2 — and is the trap
#: `pd_pose.py` already warns about.)
GUN_UNITS_PER_METRE = UNITS_PER_METRE

#: The barrel axis of a third-person gun model, as a unit vector.
#:
#: `DESIGN_PD_SIMULANT_AI.md` §15 had to infer this by measuring a convention,
#: because it is not recoverable from the mesh. It does not have to be inferred:
#: the `CHRGUNFIRE` node authors the muzzle position, and across **all 27** chr
#: gun models that carry one, the dominant component is negative X — unanimously,
#: with no exceptions to explain away. `chr_get_gun_pos` (`chraction.c:9640`)
#: reads the same node for the shot ray, which is why flash and bullet always
#: agree on screen.
GUN_BARREL_AXIS = (-1.0, 0.0, 0.0)

#: The fixed light PD's normal-lit gun parts are baked against, matching the world
#: shader's own legacy directional look (`shader.wgsl`'s `shade()`:
#: `l = normalize(0.4, 1.0, 0.6)`, `0.25 + 0.75 * abs(dot(n, l))`).
#:
#: Why bake instead of light at runtime: the viewmodel shader is deliberately UNLIT
#: (`shader_viewmodel.wgsl` — "the GoldenEye weapon skins are N64-style, no
#: lighting"), and the GoldenEye guns look shaded only because their `COLOR_0`
#: carries shading baked in. PD's normal-table parts were lit by the RSP at runtime
#: instead, so nothing in our pipeline shades them and they render dead flat. For a
#: static viewmodel, pre-lighting the authored normals into `COLOR_0` gives the same
#: result as lighting them each frame, needs no engine change, and matches how PD's
#: OTHER parts already ship their shading. The `NORMAL` attribute is still exported,
#: so a future lit path has real normals to use.
GUN_BAKE_LIGHT = (0.4, 1.0, 0.6)
GUN_BAKE_AMBIENT = 0.25


def model_parts(m: ModelDef) -> dict[int, int]:
    """`MODELPART_* -> node offset`, from the modeldef's own parts table.

    Layout per `model_get_part` (`lib/model.c:327`): `numparts` node pointers at
    `modeldef.parts`, followed immediately by a sorted `s16 partnums[numparts]`
    that the engine binary-searches. The part numbers are NOT the `part` field in
    a `POSITION` node's rodata — that is the *animation* part — which is why
    looking for `MUZZLEPOS` there finds nothing on a first-person gun.
    """
    out: dict[int, int] = {}
    if m.numparts <= 0 or not seg_ok(m.parts):
        return out
    base = seg_off(m.parts)
    nums_base = base + 4 * m.numparts
    if nums_base + 2 * m.numparts > len(m.data):
        return out
    for i in range(m.numparts):
        (ptr,) = struct.unpack_from(">I", m.data, base + 4 * i)
        (partnum,) = struct.unpack_from(">h", m.data, nums_base + 2 * i)
        if seg_ok(ptr) and partnum >= 0:
            out.setdefault(partnum, seg_off(ptr))
    return out


def gun_part_offsets(m: ModelDef) -> dict[int, tuple[float, float, float]]:
    """Accumulated rest translation per matrix slot, for baking a static mesh.

    A character gets this for free: its `POSITION` nodes become glTF joints and
    the node hierarchy composes them. A gun has no rig here — we want one static
    mesh — so the same parent-relative translations have to be summed down the
    tree and folded into the vertices, or every part piles up at the origin.
    """
    nodes = {n.offset: n for n in m.walk()}
    local: dict[int, tuple[float, float, float]] = {}
    slots: dict[int, list[int]] = {}

    for node in nodes.values():
        t = node.type & 0xFF
        if t == NODE_POSITION_T and seg_ok(node.rodata):
            ro = seg_off(node.rodata)
            x, y, z, _part, mi0, mi1, mi2 = struct.unpack_from(">fffHhhh", m.data, ro)
            local[node.offset] = (x, y, z)
            slots[node.offset] = [i for i in (mi0, mi1, mi2) if i >= 0]
        elif t == NODE_POSITIONHELD and seg_ok(node.rodata):
            ro = seg_off(node.rodata)
            x, y, z, mi = struct.unpack_from(">fffh", m.data, ro)
            local[node.offset] = (x, y, z)
            slots[node.offset] = [mi] if mi >= 0 else []

    def accumulated(off: int) -> tuple[float, float, float]:
        total = [0.0, 0.0, 0.0]
        cur = nodes.get(off)
        seen = set()
        while cur is not None and cur.offset not in seen:
            seen.add(cur.offset)
            t = local.get(cur.offset)
            if t is not None:
                total[0] += t[0]
                total[1] += t[1]
                total[2] += t[2]
            cur = nodes.get(seg_off(cur.parent)) if seg_ok(cur.parent) else None
        return (total[0], total[1], total[2])

    out: dict[int, tuple[float, float, float]] = {}
    for off, mis in slots.items():
        acc = accumulated(off)
        for mi in mis:
            out.setdefault(mi, acc)
    return out


def gun_metadata(m: ModelDef, scale: float) -> dict:
    """The authored nodes an engine needs off a gun model.

    `CHRGUNFIRE` (`modelrodata_chrgunfire`, `types.h:502`) is the important one:
    a position, a size in three axes, and a texture. It is BOTH the muzzle-flash
    placement and the barrel origin for the shot ray, so taking it means flash
    and bullet agree by construction rather than by tuning.

    `POSITIONHELD` (`types.h:536`) is the grip offset applied between a holder's
    hand matrix and the gun geometry — the reason PD needs no per-weapon,
    per-character attach tuning at all (`DESIGN_PD_WEAPON_MECHANICS.md` §2).
    """
    meta: dict = {"muzzle": None, "muzzle_dim": None, "hold": None, "star_flash": False}

    # The FIRST-PERSON models have no CHRGUNFIRE at all — they place the flash and
    # the grip with named `POSITION` parts instead (`MODELPART_GUN_MUZZLEPOS` 0x32
    # / `HOLDPOS` 0x37, `constants.h:2418-2424`). This is the authored version of
    # the single shared `DEFAULT_MUZZLE` that `combat/config.rs` applies to all 24
    # weapons, so it is worth reading even though the two model families use
    # different mechanisms for the same job.
    nodes = {n.offset: n for n in m.walk()}
    parts = model_parts(m)

    def accumulated_offset(off: int) -> tuple[float, float, float]:
        """Sum the POSITION translations from a node up to the root."""
        total = [0.0, 0.0, 0.0]
        cur = nodes.get(off)
        seen: set[int] = set()
        while cur is not None and cur.offset not in seen:
            seen.add(cur.offset)
            if (cur.type & 0xFF) in (NODE_POSITION_T, NODE_POSITIONHELD) and seg_ok(cur.rodata):
                cro = seg_off(cur.rodata)
                cx, cy, cz = struct.unpack_from(">fff", m.data, cro)
                total[0] += cx
                total[1] += cy
                total[2] += cz
            cur = nodes.get(seg_off(cur.parent)) if seg_ok(cur.parent) else None
        return (total[0] * scale, total[1] * scale, total[2] * scale)

    if MODELPART_GUN_MUZZLEPOS in parts:
        meta["muzzle"] = list(accumulated_offset(parts[MODELPART_GUN_MUZZLEPOS]))
        meta["muzzle_from"] = "MODELPART_GUN_MUZZLEPOS"
    if MODELPART_GUN_HOLDPOS in parts:
        meta["hold"] = list(accumulated_offset(parts[MODELPART_GUN_HOLDPOS]))

    for node in m.walk():
        t = node.type & 0xFF
        # CHRGUNFIRE wins where both exist: on a third-person model it is the
        # authoritative flash AND shot origin (`chr_get_gun_pos`).
        if t == NODE_CHRGUNFIRE and seg_ok(node.rodata) and meta.get("muzzle_from") != "CHRGUNFIRE":
            ro = seg_off(node.rodata)
            px, py, pz, dx, dy, dz = struct.unpack_from(">ffffff", m.data, ro)
            meta["muzzle"] = [px * scale, py * scale, pz * scale]
            meta["muzzle_dim"] = [dx * scale, dy * scale, dz * scale]
            meta["muzzle_from"] = "CHRGUNFIRE"
        elif t == NODE_STARGUNFIRE:
            # A handful of weapons use the star-shaped flash instead (AR34,
            # Cyclone, Dragon, Avenger). Recorded so a renderer can tell.
            meta["star_flash"] = True
        elif t == NODE_POSITIONHELD and seg_ok(node.rodata) and meta["hold"] is None:
            ro = seg_off(node.rodata)
            x, y, z, _mi = struct.unpack_from(">fffh", m.data, ro)
            meta["hold"] = [x * scale, y * scale, z * scale]

    # PD's OWN fallback, not an invention of ours. `chr_get_gun_pos`
    # (`chraction.c:9640`) is a two-tier lookup:
    #
    #   MODELPART_0000 (the CHRGUNFIRE node) -> fire from its authored `pos`
    #   else MODELPART_0001 (POSITIONHELD)   -> fire from that node's translation
    #
    # 17 of our 33 third-person models genuinely carry no CHRGUNFIRE — the data
    # is absent, not missed — and for those PD shoots from the **grip point**,
    # with no barrel offset at all. Two independent signals agree on the split:
    # every model with a CHRGUNFIRE exposes part 0x0, and every model without one
    # exposes part 0x1 instead. So this is the authored answer to the barrel-origin
    # question, and the weapons that look like they lack one do not need a guess.
    if meta["muzzle"] is None:
        if MODELPART_0000 in parts:
            meta["muzzle"] = list(accumulated_offset(parts[MODELPART_0000]))
            meta["muzzle_from"] = "MODELPART_0000"
        elif MODELPART_0001 in parts:
            meta["muzzle"] = list(accumulated_offset(parts[MODELPART_0001]))
            meta["muzzle_from"] = "MODELPART_0001 (grip — PD's own fallback)"
        elif meta["hold"] is not None:
            # No parts table at all (the thrown items: grenade, mines, knife).
            # Same rule, reached through the node rather than the parts array.
            meta["muzzle"] = list(meta["hold"])
            meta["muzzle_from"] = "POSITIONHELD (grip — no parts table)"
    return meta


def dl_vertex_table(
    m: ModelDef, node_offset: int
) -> tuple[str, list[tuple[float, float, float, float]]]:
    """The per-vertex table a drawable node indexes with its `Vtx.colour` byte.

    Returns `(kind, entries)` where `kind` is `"normal"`, `"colour"` or `""`.

    PD calls this field `colours` (`modelrodata_dl.colours` / `numcolours`,
    `types.h:552`) — and **it is both**, per node. That is the standard F3DEX dual
    use of the vertex colour slot: with `G_LIGHTING` enabled the RSP reads those
    three bytes as a normal, without it as a colour. Measured across the 33 guns'
    174 drawable nodes: **120 hold colours and 54 hold normals**, so neither reading
    alone is right and assuming either one silently wrecks the other's meshes.

    The two are easy to tell apart from the data, which is how this is classified
    rather than guessed:

      * a **normal** table's entries are unit length as signed bytes — `(127,0,0)`
        is +X, `(0,129,0)` is −Y, `(89,167,0)` is (0.7, −0.7, 0);
      * a **colour** table's entries have equal components — `(48,48,48)`,
        `(68,68,68)`, `(136,136,136)` — i.e. greyscale, which is PD's baked
        shading, meant to multiply onto the texture exactly the way the GoldenEye
        weapon GLBs do (see `engine::assets::textured_model`'s own note that "a
        white palette entry tinted by the vertex color gives the real dark metal
        surface color").

    Discarding this is why the guns rendered flat: the loader defaults a missing
    `COLOR_0` to white (so no shading) and a missing `NORMAL` to a single constant
    `(0,1,0)` (so no form, and the viewmodel matcap collapses to one texel).

    `Vtx.colour` is a **byte offset** into the table, not an element index — the
    observed values run 0, 4, 8 … 252, i.e. 64 entries of 4 bytes.

    Two layouts, because the node types differ:
      * `DL` (0x18) carries a real `colours` pointer at 0x08 and `numcolours` at 0x16.
      * `GUNDL` (0x04) has neither, so its table follows the vertex array — the same
        segment-6 convention `pd_model._run_dl` already assumes.
    """
    node = m.read_node(node_offset)
    t = node.type & 0xFF
    if t not in (0x18, 0x04) or not seg_ok(node.rodata):
        return "", []
    ro = seg_off(node.rodata)
    if ro + 0x18 > len(m.data):
        return "", []
    vtx_addr, numvertices = struct.unpack_from(">Ih", m.data, ro + 0x0C)
    if not seg_ok(vtx_addr) or numvertices <= 0:
        return "", []

    if t == 0x18:
        (col_addr,) = struct.unpack_from(">I", m.data, ro + 0x08)
        (numcolours,) = struct.unpack_from(">H", m.data, ro + 0x16)
        base = (
            seg_off(col_addr) if seg_ok(col_addr) else seg_off(vtx_addr) + numvertices * VTX_SIZE
        )
        count = numcolours if numcolours > 0 else 64
    else:
        base = seg_off(vtx_addr) + numvertices * VTX_SIZE
        count = 64  # GUNDL declares no count; 64 covers the observed 0..252 offsets

    raw: list[tuple[int, int, int, int]] = []
    for i in range(count):
        off = base + i * 4
        if off + 4 > len(m.data):
            break
        raw.append(tuple(m.data[off + k] for k in range(4)))  # type: ignore[arg-type]
    if not raw:
        return "", []

    # Classify from the entries that are actually non-null.
    def signed(b: int) -> int:
        return b - 256 if b > 127 else b

    live = [e for e in raw if e[:3] != (0, 0, 0)]
    if not live:
        return "", []
    grey = sum(1 for e in live if e[0] == e[1] == e[2])
    unit = 0
    for e in live:
        x, y, z = (signed(e[k]) / 127.0 for k in range(3))
        if abs((x * x + y * y + z * z) ** 0.5 - 1.0) <= 0.06:
            unit += 1
    if grey > len(live) * 0.5 or unit <= len(live) * 0.5:
        # Colours: 0-255 straight through, alpha included.
        return "colour", [tuple(c / 255.0 for c in e) for e in raw]  # type: ignore[misc]
    return "normal", [
        (signed(e[0]) / 127.0, signed(e[1]) / 127.0, signed(e[2]) / 127.0, 1.0) for e in raw
    ]


def part_subtree_nodes(m: ModelDef, parts: list[int]) -> set[int]:
    """Every node offset at or under the given `MODELPART_*` numbers.

    Needed because a part is a `POSITION`/`TOGGLE` node and the geometry hangs
    below it, so hiding a part means dropping its whole subtree rather than one
    node.
    """
    nodes = {n.offset: n for n in m.walk()}
    table = model_parts(m)
    roots = [table[p] for p in parts if p in table]
    if not roots:
        return set()

    # Walk parents upward for every node and mark it if any ancestor is a root.
    rootset = set(roots)
    out: set[int] = set()
    for off in nodes:
        cur = nodes.get(off)
        seen: set[int] = set()
        while cur is not None and cur.offset not in seen:
            if cur.offset in rootset:
                out.add(off)
                break
            seen.add(cur.offset)
            cur = nodes.get(seg_off(cur.parent)) if seg_ok(cur.parent) else None
    return out


def export_gun(
    path: str,
    out: str,
    scale: float,
    hide_parts: list[int] | None = None,
    only_parts: list[int] | None = None,
) -> dict:
    """Export one PD weapon model as a static, textured GLB.

    Deliberately **not** skinned. PD's first-person models articulate (a slide,
    a magazine, 43 matrices on the Falcon 2) driven by the `guncmd` bytecode, but
    our viewmodel is a single mesh with a recoil kick, so there is nowhere for
    those parts to land yet. Baking the rest pose into one mesh is the honest
    version of what we can actually draw; the part offsets are preserved in the
    geometry rather than thrown away, so adding articulation later is a re-export
    and not a re-rip.
    """
    model = load(path)
    groups = model.geometry()
    if not groups:
        raise SystemExit(f"{model.name}: no geometry")

    # Apply the weapon's authored `modelpartvisibility`. Three weapons can share one
    # model file and look different purely through this list — the plain Falcon 2
    # hides its scope, silencer, both magazines and MUZZLEFLASH1 — so exporting
    # every part gives a pistol wearing its scope AND its silencer, with the muzzle
    # flash geometry stuck on permanently as a square.
    if hide_parts:
        hidden = part_subtree_nodes(model, hide_parts)
        kept = [g for g in groups if g.node_offset not in hidden]
        if kept:
            groups = kept
    # `only_parts` is the inverse, for pulling a single part out into its own GLB
    # (the muzzle flash, so it can be drawn on fire instead of always).
    if only_parts is not None:
        wanted = part_subtree_nodes(model, only_parts)
        groups = [g for g in groups if g.node_offset in wanted]
        if not groups:
            raise SystemExit(f"{model.name}: no geometry for parts {only_parts}")

    offsets = gun_part_offsets(model)
    meta = gun_metadata(model, scale)
    cfgs = read_texconfigs(model)
    # One vertex table per drawable node — PD stores them per DL, not per model, and
    # each is either normals or colours (see `dl_vertex_table`).
    vtables = {g.node_offset: dl_vertex_table(model, g.node_offset) for g in groups}

    glb = Glb("pd_gltf.py (Perfect Dark -> glTF)")

    # Same texture batching as the character path: one primitive per PD texture,
    # which is how PD draws it and what the renderer wants.
    batches: dict[object, list] = {}
    untextured = 0
    flat_tris = 0
    undecodable: set[int] = set()
    # Decoded once per texture, from PD's own data or the editor dump
    # ([`resolve_texture`]): `texnum -> (w, h, rgba, source)`.
    images: dict[int, tuple[int, int, bytes, str]] = {}
    for gi, g in enumerate(groups):
        for a, b, c, tex in g.tris:
            cfg = cfgs.get(tex)
            if cfg is not None and tex not in undecodable and tex not in images:
                try:
                    images[tex] = resolve_texture(model, cfg, tex)
                except pd_tex.UnsupportedTexture as e:
                    print(f"  NOTE: {model.name} texture {tex:#x}: {e}", file=sys.stderr)
                    undecodable.add(tex)
            if cfg is not None and tex in undecodable:
                cfg = None
            # Three states, not two. A triangle whose texture our codec cannot decode
            # keeps the loud debug palette — that is data we are MISSING. But PD also
            # authors triangles with **no texture at all** (`tex == -1`, the display
            # list simply never binds one; the Mauler has six), and those are shaded
            # by the vertex colour alone on the N64. Painting them from the palette
            # put a garish stripe on an otherwise finished gun and read as "the
            # Mauler looks very off" — so they get a white texel and their own
            # authored shading, which is what PD draws.
            flat = cfg is None and tex not in cfgs
            if cfg is None and not flat:
                untextured += 1
            elif flat:
                flat_tris += 1
            key = tex if cfg is not None else ("flat" if flat else None)
            batch = batches.get(key)
            if batch is None:
                batch = batches[key] = [cfg, gi, {}, [], [], [], [], []]
            _, _, remap, pos, uv, idx, nrm, col = batch
            for corner in (a, b, c):
                local = (gi, corner)
                out_i = remap.get(local)
                if out_i is None:
                    v = g.verts[corner]
                    ox, oy, oz = offsets.get(v.mtx, (0.0, 0.0, 0.0))
                    out_i = remap[local] = len(pos) // 3
                    pos.extend(
                        (
                            (v.x + ox) * scale,
                            (v.y + oy) * scale,
                            (v.z + oz) * scale,
                        )
                    )
                    if cfg is not None:
                        # PD texcoords are S10.5 fixed point in TEXELS, so they
                        # normalise by the size of the image actually being shipped
                        # — which is not always the texconfig's. Texture 0x606 is
                        # 4x16 there and 8x16 in the editor's export (the texconfig
                        # width is what the gun *uses*, not what the pool holds), and
                        # dividing those texel counts by 4 would stretch the used
                        # half across the whole image.
                        iw, ih, *_ = images[tex]
                        uv.extend((v.s / 32.0 / iw, v.t / 32.0 / ih))
                    elif flat:
                        uv.extend((0.5, 0.5))  # the single white texel
                    else:
                        uv.extend(palette_uv(gi, len(groups)))
                    # The authored per-vertex datum. `Vtx.colour` is a byte offset,
                    # so /4. A normals node feeds NORMAL and leaves the colour white;
                    # a colours node feeds COLOR_0 and leaves the normal to be
                    # computed geometrically below.
                    kind, table = vtables.get(g.node_offset, ("", []))
                    ti = v.colour // 4
                    e = table[ti] if ti < len(table) else None
                    if kind == "normal" and e is not None:
                        nrm.extend(e[:3])
                        # Pre-light the authored normal into the vertex colour (see
                        # GUN_BAKE_LIGHT). `abs` so a face wound the other way is lit
                        # rather than black — PD's winding is not consistent, and a
                        # two-sided term is what the world shader uses too.
                        ln = sum(c * c for c in GUN_BAKE_LIGHT) ** 0.5
                        ndl = abs(sum(e[k] * GUN_BAKE_LIGHT[k] / ln for k in range(3)))
                        sh = GUN_BAKE_AMBIENT + (1.0 - GUN_BAKE_AMBIENT) * min(ndl, 1.0)
                        col.extend((sh, sh, sh, 1.0))
                    elif kind == "colour" and e is not None:
                        nrm.extend((0.0, 0.0, 0.0))
                        # PD's null slot is (0,0,0,0) and means "unspecified", NOT
                        # black and NOT transparent. Passing it through multiplies the
                        # whole mesh to nothing — the Remote Mine's entire model
                        # indexes it, so it rendered pure black.
                        if e[0] == 0.0 and e[1] == 0.0 and e[2] == 0.0:
                            col.extend((1.0, 1.0, 1.0, 1.0))
                        else:
                            a = e[3] if e[3] > 0.0 else 1.0
                            col.extend((e[0], e[1], e[2], a))
                    else:
                        nrm.extend((0.0, 0.0, 0.0))
                        col.extend((1.0, 1.0, 1.0, 1.0))
                idx.append(out_i)
    if untextured:
        print(
            f"  WARNING: {model.name}: {untextured} triangles reference a texture we "
            "cannot decode — they fall back to the per-part debug palette",
            file=sys.stderr,
        )
    if flat_tris:
        print(
            f"  note: {model.name}: {flat_tris} triangles are authored untextured — "
            "drawn from their vertex colour",
            file=sys.stderr,
        )

    # PD leaves some vertices pointing at table entry 0, which is a null
    # (0,0,0,0) rather than a direction — 92 of the Falcon 2's 802. Shipping those
    # as a zero vector would hand the shader `normalize(0)` and produce NaN, so
    # they fall back to the geometric normal of the faces they belong to. Only the
    # null ones: an authored normal is always preferred over a computed one.
    for _cfg, _gi, _remap, pos, _uv, idx, nrm, _col in batches.values():
        missing = {
            i for i in range(len(pos) // 3) if nrm[i * 3] == 0.0 and nrm[i * 3 + 1] == 0.0 and nrm[i * 3 + 2] == 0.0
        }
        if not missing:
            continue
        acc = {i: [0.0, 0.0, 0.0] for i in missing}
        for k in range(0, len(idx) - 2, 3):
            a, b, c = idx[k], idx[k + 1], idx[k + 2]
            if not (a in acc or b in acc or c in acc):
                continue
            pa = pos[a * 3 : a * 3 + 3]
            pb = pos[b * 3 : b * 3 + 3]
            pc = pos[c * 3 : c * 3 + 3]
            u = [pb[j] - pa[j] for j in range(3)]
            v = [pc[j] - pa[j] for j in range(3)]
            fn = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
            for vi in (a, b, c):
                if vi in acc:
                    for j in range(3):
                        acc[vi][j] += fn[j]
        for i, n in acc.items():
            ln = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) ** 0.5
            if ln > 1e-9:
                nrm[i * 3], nrm[i * 3 + 1], nrm[i * 3 + 2] = n[0] / ln, n[1] / ln, n[2] / ln
            else:
                nrm[i * 3 + 1] = 1.0  # degenerate face — anything unit beats NaN

    sampler = glb._append("samplers", {"magFilter": 9728, "minFilter": 9728})
    palette_image = None
    primitives = []
    nverts = 0
    ntris = 0
    white_image = None
    from_editor = 0
    for texnum, (cfg, gi, _remap, pos, uv, idx, nrm, col) in batches.items():
        hints = ""
        if texnum == "flat":
            # Authored untextured (see the batching above): one white texel, so the
            # shader's `texel x vertex colour` is the vertex colour.
            if white_image is None:
                white_image = glb.image_png(png_bytes(1, 1, bytes((255, 255, 255, 255))))
            image = white_image
            mat_name = f"part{gi:02d}_vertexcolour"
        elif cfg is not None:
            tw, th, rgba, src = images[texnum]
            from_editor += src == "editor"
            image = glb.image_png(png_bytes(tw, th, rgba))
            # The editor's render intent, carried on the material NAME because that
            # is the convention our engine reads: `textured_model::load` binds the
            # `*EnvMapping*` material's texture as the reflection map, and
            # `combat::load_flash` keeps the `CullBoth` geometry. Naming the texture
            # by its POOL number (not the texconfig slot) keeps a material traceable
            # to `pd dump/weapons/*/tempImgEd<NNNN>.bmp`.
            hints = editor_textures().get(texnum, {}).get("hints", "")
            mat_name = f"tex{texnum:04x}_{tw}x{th}" + (f"_{hints}" if hints else "")
        else:
            if palette_image is None:
                palette_image = glb.image_png(palette_png(len(groups)))
            image = palette_image
            mat_name = f"part{gi:02d}_untextured"
        texture = glb._append("textures", {"sampler": sampler, "source": image})
        material = glb._append(
            "materials",
            {
                "name": mat_name,
                "pbrMetallicRoughness": {
                    "baseColorTexture": {"index": texture},
                    # An `EnvMapping` face is painted by the REFLECTION ALONE, so its
                    # flat base contributes nothing. On the N64 those faces generate
                    # their texcoords from the vertex normal (`G_TEXTURE_GEN`), which
                    # means the s/t stored in the vertex is leftover data and sampling
                    # the map with it — as we did — paints a tiled blue-grey smear
                    # flat across the gun. That is exactly the "dull metal" symptom.
                    # The engine's matcap (`shader_viewmodel.wgsl`) is the *correct*
                    # lookup for these faces, so the base is zeroed and the reflection
                    # left to do the whole job, rather than the two being added.
                    "baseColorFactor": [0.0, 0.0, 0.0, 1.0]
                    if "EnvMapping" in hints
                    else [1.0, 1.0, 1.0, 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 1.0,
                },
                # RGBA5551's single alpha bit is all-or-nothing, which is what MASK
                # means — but the editor's BMPs carry a real 8-bit ramp (the muzzle
                # flames fade out over ~8 texels), and cutting that at 0.5 would
                # give back the hard-edged square this pass exists to remove. Our
                # own renderer reads neither: the gun pass is opaque and the flash
                # pass is additive on `SrcAlpha`. This is for external viewers and
                # `pd_preview.py`.
                "alphaMode": "BLEND" if "Transparent" in hints else "MASK",
                "alphaCutoff": 0.5,
                "doubleSided": True,
            },
        )
        n = len(pos) // 3
        primitives.append(
            {
                "attributes": {
                    "POSITION": glb.accessor(pos, CT_F32, "VEC3", TARGET_ARRAY, minmax=True),
                    "NORMAL": glb.accessor(nrm, CT_F32, "VEC3", TARGET_ARRAY),
                    "TEXCOORD_0": glb.accessor(uv, CT_F32, "VEC2", TARGET_ARRAY),
                    "COLOR_0": glb.accessor(col, CT_F32, "VEC4", TARGET_ARRAY),
                },
                "indices": glb.accessor(
                    idx, CT_U16 if n <= 0xFFFF else CT_U32, "SCALAR", TARGET_ELEMENT
                ),
                "material": material,
            }
        )
        nverts += n
        ntris += len(idx) // 3

    mesh = glb._append("meshes", {"name": model.name, "primitives": primitives})
    mesh_node = glb._append("nodes", {"name": "gun", "mesh": mesh})
    glb.doc["scenes"][0]["nodes"].append(mesh_node)

    # The authored attach points ride along as empty nodes AND as `extras`. The
    # nodes make them visible in any glTF viewer (so a wrong muzzle is something
    # you can see rather than something you infer from a bad tracer); the extras
    # are what a loader reads.
    if meta["muzzle"] is not None:
        n = glb._append("nodes", {"name": "MUZZLE", "translation": meta["muzzle"]})
        glb.doc["scenes"][0]["nodes"].append(n)
    if meta["hold"] is not None:
        n = glb._append("nodes", {"name": "HOLD", "translation": meta["hold"]})
        glb.doc["scenes"][0]["nodes"].append(n)
    glb.doc.setdefault("extras", {})["pd_gun"] = {
        "source": os.path.basename(path),
        "muzzle": meta["muzzle"],
        "muzzle_dim": meta["muzzle_dim"],
        "hold": meta["hold"],
        "star_flash": meta["star_flash"],
        "barrel_axis": list(GUN_BARREL_AXIS),
        "units_per_metre": GUN_UNITS_PER_METRE,
    }

    glb.save(out)
    stats = {
        "model": model.name,
        "verts": nverts,
        "tris": ntris,
        "parts": len(groups),
        "textures": sum(1 for cfg, *_ in batches.values() if cfg is not None),
        #: How many of those came from the editor dump rather than PD's own data —
        #: i.e. how much of this gun our texture codec cannot reach on its own.
        "editor_textures": from_editor,
        "palette_tris": untextured,
        **meta,
    }
    muzzle = (
        "muzzle ({:.3f},{:.3f},{:.3f})".format(*meta["muzzle"])
        if meta["muzzle"]
        else "NO MUZZLE NODE"
    )
    print(
        f"{model.name} -> {out}: {stats['verts']} verts, {stats['tris']} tris, "
        f"{stats['textures']} textures"
        + (f" ({from_editor} from the editor dump)" if from_editor else "")
        + f", {muzzle}"
    )
    return stats


def export_guns(manifest_path: str, outdir: str, scale: float) -> int:
    """Batch-export every gun named by `pd_weapons.py`'s table.

    The manifest is the generated `pd_weapons.json`, so the roster of guns is the
    decomp's own MP set rather than a hand-kept list that can drift from the
    weapon table it has to line up with.
    """
    with open(manifest_path, encoding="utf-8") as fh:
        table = json.load(fh)

    files_root = os.path.join(assets_root(), "files")
    os.makedirs(outdir, exist_ok=True)

    index: dict[str, dict] = {}
    failures: list[str] = []
    no_muzzle: list[str] = []
    grip_muzzle: list[str] = []
    flashes: list[str] = []
    for w in table["weapons"]:
        if w.get("equipment_only"):
            continue
        entry: dict = {"name": w["name_text"], "mp_index": w["mp_index"]}
        # The parts this weapon variant hides. PD has TWO tables and a static export
        # has to honour both: `modelpartvisibility` (per variant) and the
        # unconditional rows of `gunviscmds` (see `pd_weapons.py`'s `expand_gunvis`).
        # The second adds exactly two groups across the arsenal — the silenced
        # Falcon 2's 002F and the CMP150's open cartridge flap — but an open flap on
        # a holstered-looking gun is precisely the kind of "looks off" that is
        # invisible until someone names it.
        hide = sorted(
            {r["part"] for r in (w.get("part_visibility") or []) if not r.get("visible", True)}
            | {
                r["part"]
                for r in (w.get("gun_vis") or [])
                if r.get("op") == "hidden" and r.get("condition") == "always" and r.get("part")
            }
        )
        for role, key in (("fp", "fp_model"), ("tp", "tp_model")):
            rel = w["assets"].get(key)
            if not rel:
                continue
            slug = f"{w['mp_index']:02x}-{slugify(w['name_text'])}-{role}"
            src_path = os.path.join(files_root, rel.replace("/", os.sep))
            out_path = os.path.join(outdir, slug + ".glb")
            try:
                # Visibility is authored for the FIRST-PERSON model's parts; the
                # third-person `chr*` models have one geometry group and no variants.
                stats = export_gun(
                    src_path, out_path, scale, hide_parts=hide if role == "fp" else None
                )
            except SystemExit as e:
                print(f"  FAILED {rel}: {e}", file=sys.stderr)
                failures.append(f"{w['name_text']} ({role})")
                continue
            # Pull the muzzle flash into its own GLB so it can be drawn on fire.
            if role == "fp" and MODELPART_GUN_MUZZLEFLASH1 in hide:
                flash_path = os.path.join(outdir, slug.replace("-fp", "-flash") + ".glb")
                try:
                    export_gun(
                        src_path,
                        flash_path,
                        scale,
                        only_parts=[MODELPART_GUN_MUZZLEFLASH1],
                    )
                    entry["flash"] = os.path.basename(flash_path)
                    flashes.append(w["name_text"])
                except SystemExit:
                    pass  # no flash geometry — the weapon simply has none
            entry[role] = {
                "glb": slug + ".glb",
                "source": rel,
                "muzzle": stats["muzzle"],
                "muzzle_from": stats.get("muzzle_from"),
                "hold": stats["hold"],
                "star_flash": stats["star_flash"],
                "verts": stats["verts"],
                "tris": stats["tris"],
            }
            if role == "tp":
                if stats["muzzle"] is None:
                    no_muzzle.append(w["name_text"])
                elif "grip" in (stats.get("muzzle_from") or ""):
                    grip_muzzle.append(w["name_text"])
        index[str(w["mp_index"])] = entry

    with open(os.path.join(outdir, "guns.json"), "w", encoding="utf-8") as fh:
        json.dump(
            {
                "_comment": [
                    "GENERATED by pd_gltf.py guns — do not hand-edit.",
                    "Keyed by MPWEAPON index, matching combat/pd_weapons.rs.",
                    "`muzzle` is the CHRGUNFIRE node: muzzle-flash placement AND the",
                    "barrel origin for the shot ray, in engine units. `hold` is the",
                    "POSITIONHELD grip offset. The barrel points down -X on every one",
                    "of the 27 chr gun models that has a CHRGUNFIRE node.",
                ],
                "barrel_axis": list(GUN_BARREL_AXIS),
                "guns": index,
            },
            fh,
            indent=2,
        )

    print(f"\n{len(index)} guns -> {outdir}")
    if flashes:
        print(f"  {len(flashes)} carry a separate muzzle-flash GLB (shown only on fire)")
    if grip_muzzle:
        # Not a gap: `chr_get_gun_pos` falls back to the grip for these, so this IS
        # PD's answer for where the shot leaves the weapon.
        print(
            f"  {len(grip_muzzle)} fire from the grip — no CHRGUNFIRE authored, PD's own "
            f"MODELPART_0001 fallback"
        )
    if no_muzzle:
        print(f"  NO firing origin resolved at all: {', '.join(no_muzzle)}")
    if failures:
        print(f"  FAILED: {', '.join(failures)}", file=sys.stderr)
        return 1
    return 0


def slugify(name: str) -> str:
    out = "".join(c.lower() if c.isalnum() else "-" for c in name)
    while "--" in out:
        out = out.replace("--", "-")
    return out.strip("-")


# ---------------------------------------------------------------------------
# clip: animation-only GLB
# ---------------------------------------------------------------------------


def export_clip(
    anim_id: str,
    out: str,
    ref_model: str,
    scale: float,
    fps: float = DEFAULT_FPS,
    blend_joints: bool = True,
    name: str | None = None,
) -> dict:
    """Export one PD animation as a clip-only GLB bound to the shared rig.

    A reference character supplies the rig (bone names, rest offsets, which parts
    have blend joints). Every character on the 15-bone rig produces the same joint
    *names*, which is all `clip.rs` binds against, so one export drives them all —
    the rest offsets only matter for the joints whose translation this clip does
    not animate, and those come from the body's own skeleton at runtime anyway.
    """
    model = load(ref_model)
    rig = Rig(model, blend_joints=blend_joints)
    anim = load_animation(anim_id)
    if anim.numframes <= 0:
        raise SystemExit(f"{anim.id} has no frames")

    glb = Glb("pd_gltf.py (Perfect Dark -> glTF)")
    node_of = rig_nodes(glb, rig, scale)

    times = [i / fps for i in range(anim.numframes)]
    a_time = glb.accessor(times, CT_F32, "SCALAR", minmax=True)

    # Sample every part once for the whole clip, then split into channels.
    rots: list[list[float]] = [[] for _ in rig.joints]
    trans: list[list[float]] = [[] for _ in rig.joints]
    animates_translation = [False] * len(rig.joints)
    turned = 0.0
    for frame in range(anim.numframes):
        for i, j in enumerate(rig.joints):
            rot, tra = anim.part_transform(j.part, frame)
            # **Root motion** (`ANIMFIELD_08`) is a separate channel the game reads
            # with `anim_get_pos_angle_as_int` and feeds into the model transform
            # (`model.c:1856`) — it is where the clip's travel lives. Without it a
            # death rotates into a heap while its hips stay at standing height; with
            # it the pelvis drops 1083 mm -> 209 mm and the body lands. It only ever
            # appears on part 0, whose rest offset is the origin, and a blend joint
            # is the owning bone's *sibling*, so adding it to both is not a
            # double-count.
            mx, my, mz, angle = anim.part_motion(j.part, frame)
            turned = max(turned, abs(angle))
            q = quat_from_pd_rotation(*rot)
            if j.is_blend:
                q = quat_halfway(q)
            rots[i] += list(q)
            if tra != (0.0, 0.0, 0.0) or (mx, my, mz) != (0.0, 0.0, 0.0):
                animates_translation[i] = True
            # `model.c:1158` — the animation translation is ADDED to the node's
            # rest offset, it does not replace it.
            trans[i] += [
                (j.offset[0] + tra[0] + mx) * scale,
                (j.offset[1] + tra[1] + my) * scale,
                (j.offset[2] + tra[2] + mz) * scale,
            ]

    samplers: list[dict] = []
    channels: list[dict] = []
    for i, j in enumerate(rig.joints):
        s = len(samplers)
        samplers.append(
            {"input": a_time, "output": glb.accessor(rots[i], CT_F32, "VEC4"), "interpolation": "LINEAR"}
        )
        channels.append({"sampler": s, "target": {"node": node_of[i], "path": "rotation"}})
        # Only emit translation where the clip actually moves the joint; otherwise
        # the bone's bind translation (its rest offset) already says the same
        # thing, and `clip.rs` composes against it.
        if animates_translation[i]:
            s = len(samplers)
            samplers.append(
                {
                    "input": a_time,
                    "output": glb.accessor(trans[i], CT_F32, "VEC3"),
                    "interpolation": "LINEAR",
                }
            )
            channels.append({"sampler": s, "target": {"node": node_of[i], "path": "translation"}})

    glb._append(
        "animations",
        {"name": name or anim.id, "samplers": samplers, "channels": channels},
    )
    glb.save(out)

    moved = sum(1 for i in range(len(rig.joints)) if animates_translation[i])
    duration = (anim.numframes - 1) / fps
    print(
        f"{anim.id} -> {out}: {anim.numframes} frames, {duration:.2f}s @ {fps:g}fps, "
        f"{len(channels)} channels ({moved} translated)"
    )
    # The root-motion field's fourth channel turns the whole character. The engine
    # owns a hunter's facing, so it is deliberately NOT baked into the clip — say so
    # if a clip actually uses it rather than dropping it silently.
    if turned > 1e-3:
        print(
            f"  note: {anim.id} also turns the root by up to {math.degrees(turned):.1f}deg; "
            "that channel is not exported (the game drives facing)"
        )
    return {
        "anim": anim.id,
        "frames": anim.numframes,
        "duration": duration,
        "channels": len(channels),
    }


# ---------------------------------------------------------------------------
# batch
# ---------------------------------------------------------------------------


def export_batch(manifest_path: str, outdir: str, scale: float, blend_joints: bool) -> int:
    """Export everything named in a JSON manifest.

    A character is either a bare body name or a `[body, head]` pair; heads and
    bodies mix freely in PD (`g_HeadsAndBodies`, `game/modeldata/robot.c:64`), so
    the pairing is an authoring choice and belongs here rather than in a rule.

    ```json
    {
      "characters": {"pd_a51guard": "a51guard", "pd_joanna": ["dark_frock", "headdark_frock"]},
      "clips":      {"00-idle": "ANIM_TWO_GUN_HOLD"},
      "clip_rig":   "a51guard",
      "fps": 30
    }
    ```
    """
    with open(manifest_path, encoding="utf-8") as fh:
        man = json.load(fh)
    chrs_dir = os.path.join(assets_root(), "files", "chrs")
    fps = float(man.get("fps", DEFAULT_FPS))

    chr_out = os.path.join(outdir, "characters")
    clip_out = os.path.join(outdir, "animations")
    os.makedirs(chr_out, exist_ok=True)
    os.makedirs(clip_out, exist_ok=True)

    for name, src in man.get("characters", {}).items():
        body, head = (src, None) if isinstance(src, str) else (src[0], src[1])
        export_char(
            os.path.join(chrs_dir, body + ".bin"),
            os.path.join(chr_out, name + ".glb"),
            scale,
            blend_joints,
            os.path.join(chrs_dir, head + ".bin") if head else None,
        )
    ref = os.path.join(chrs_dir, man.get("clip_rig", "a51guard") + ".bin")
    for name, anim_id in man.get("clips", {}).items():
        export_clip(
            anim_id,
            os.path.join(clip_out, name + ".glb"),
            ref,
            scale,
            fps,
            blend_joints,
            name=name,
        )
    return 0


def assets_root() -> str:
    return os.path.join(
        os.path.dirname(os.path.abspath(__file__)),
        "..", "..", "reference", "pd-decomp", "src", "assets", "ntsc-final",
    )


# ---------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--metres", action="store_true", help="export in metres, not engine units")
    ap.add_argument(
        "--no-blend-joints",
        action="store_true",
        help="fold PD blend matrices onto their owning joint (the pd_pose.py approximation)",
    )
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("char", help="skinned character GLB")
    p.add_argument("model")
    p.add_argument("out")
    p.add_argument("--head", default=None, help="head*.bin to graft onto the HEADSPOT")

    p = sub.add_parser("clip", help="animation-only GLB")
    p.add_argument("anim")
    p.add_argument("out")
    p.add_argument("--rig", default=None, help="chr .bin supplying the rig (default a51guard)")
    p.add_argument("--fps", type=float, default=DEFAULT_FPS)
    p.add_argument("--name", default=None)

    p = sub.add_parser("batch", help="export everything in a JSON manifest")
    p.add_argument("manifest")
    p.add_argument("outdir")

    p = sub.add_parser("gun", help="static weapon GLB (first- or third-person model)")
    p.add_argument("model")
    p.add_argument("out")

    p = sub.add_parser("guns", help="every gun in pd_weapons.json, both models each")
    p.add_argument("manifest", help="the pd_weapons.py-generated pd_weapons.json")
    p.add_argument("outdir")

    args = ap.parse_args()
    scale = 1.0 / UNITS_PER_METRE if args.metres else EXPORT_SCALE
    blends = not args.no_blend_joints

    if args.cmd == "char":
        export_char(args.model, args.out, scale, blends, args.head)
        return 0
    if args.cmd == "clip":
        rig = args.rig or os.path.join(assets_root(), "files", "chrs", "a51guard.bin")
        export_clip(args.anim, args.out, rig, scale, args.fps, blends, args.name)
        return 0
    if args.cmd == "gun":
        export_gun(args.model, args.out, scale)
        return 0
    if args.cmd == "guns":
        return export_guns(args.manifest, args.outdir, scale)
    return export_batch(args.manifest, args.outdir, scale, blends)


if __name__ == "__main__":
    sys.exit(main())
