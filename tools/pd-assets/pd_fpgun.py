#!/usr/bin/env python3
"""Export Perfect Dark's FIRST-PERSON guns + hands for the `pd_guns` spike, faithfully.

The earlier export (`pd_gltf.py gun`) baked each gun into one static mesh with
materials guessed from editor-dump names. This one keeps everything PD itself
uses at runtime, so the Rust side can replay it rather than approximate it:

* **The articulated node tree.** Every `POSITION` node (anim part, matrix slot,
  rest offset), every `TOGGLE` (for `bgun_set_part_visible`), the `modeldef.parts`
  table, `POSITIONHELD` and `STARGUNFIRE`. A gun is ~43 joints: joint 0 is the
  root, 1-16 the right arm/hand, 17-32 the left, 33+ the gun's own parts
  (slide, magazine, trigger, muzzle). Measured, not assumed — see the hand note.
* **The hand model is posed by the GUN's matrices.** `bgun_render`
  (`bondgun.c:8394`) draws `hand->handmodel` with `hand->gunmodel.matrices`, and
  `bgun_init_hand_anims` (`bondgun.c:3157`) points both models at one `struct
  anim`. The hand files' own joints 0-32 are byte-identical to every gun's joints
  0-32, so a hand vertex simply names a gun matrix index. We export the hand as a
  second model whose vertices reference those shared slots.
* **The display lists, interpreted like the RSP/RDP does** (reference:
  `reference/pd-pcport/port/fast3d/gfx_pc.cpp`, which runs PD correctly). Per
  triangle batch we record the *state that drew it*: geometry mode (G_LIGHTING /
  G_TEXTURE_GEN decide whether a vertex's colour bytes are a colour or a normal —
  no heuristic classification), the two-cycle colour combiner, the blender,
  z-mode, alpha compare, and the texture tile (clamp/mirror/wrap, shifts, the
  half-texel `uls` offset from `tex_write_tile_lods`). State leaks from node to
  node inside one model render exactly as on hardware, so it is carried across.
* **`bondgun.c`'s render context is applied first**, as `model_render_node_gundl`
  does: rendermode 3 (`MODELRENDERMODE_CTXAWARE_1PASS`) installs
  `G_CC_TRILERP, G_CC_MODULATEIA2` and `G_RM_FOG_PRIM_A, G_RM_AA_ZB_OPA_SURF2`
  (`model.c:2902`), then the DL overrides what it wants.
* **Animations are shipped RAW** (header + frames, as `animations/*.bin`). The
  Rust side ports `anim_get_rot_translate_scale` itself (`anim.c:424`), including
  the repeat-frame remap `pd_anim.py` lacks.
* **The weapon tables**, via `pd_weapons.py`'s provenance-keeping C parser, plus
  what it skipped: every `struct guncmd` script, `invaimsettings`,
  `recoilsettings`, `noisesettings`, and the change-function animations.

Usage:
    python tools/pd-assets/pd_fpgun.py all  [outdir]      # default native/assets/weapons/pd_fp
    python tools/pd-assets/pd_fpgun.py model <gun.bin> <out.json>
    python tools/pd-assets/pd_fpgun.py dl <gun.bin>        # print the interpreted batches
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pd_gltf  # noqa: E402
import pd_model  # noqa: E402
import pd_tex  # noqa: E402
import pd_weapons  # noqa: E402
from pd_model import seg_off, seg_ok  # noqa: E402

from pd_paths import ASSETS, REPO, decomp_rel, src  # noqa: E402,F401

GUNS_DIR = os.path.join(ASSETS, "files", "guns")
#: Scratch output for the standalone `all`/`props` commands; build_assets.py
#: writes the real layout through pd_models.py.
DEFAULT_OUT = os.path.join(REPO, "out", "pd_fpgun")

# ---------------------------------------------------------------------------
# GBI constants (include/PR/gbi.h, include/gbiex.h, fast3d's gfx_run_dl)
# ---------------------------------------------------------------------------

G_MTX, G_MOVEMEM, G_VTX, G_DL, G_COL = 0x01, 0x03, 0x04, 0x06, 0x07
G_TRI4, G_CLEARGEOM, G_SETGEOM, G_ENDDL = 0xB1, 0xB6, 0xB7, 0xB8
G_SETOTHERMODE_L, G_SETOTHERMODE_H, G_TEXTURE, G_MOVEWORD = 0xB9, 0xBA, 0xBB, 0xBC
G_POPMTX, G_TRI1, G_SETTEXNUM = 0xBD, 0xBF, 0xC0
G_SETTILESIZE, G_LOADBLOCK, G_SETTILE, G_LOADTLUT = 0xF2, 0xF3, 0xF5, 0xF0
G_SETFOGCOLOR, G_SETBLENDCOLOR, G_SETPRIMCOLOR, G_SETENVCOLOR = 0xF8, 0xF9, 0xFA, 0xFB
G_SETCOMBINE, G_SETTIMG = 0xFC, 0xFD

GM_ZBUFFER = 0x00000001
GM_SHADE = 0x00000004
GM_SHADING_SMOOTH = 0x00000200
GM_CULL_FRONT = 0x00001000
GM_CULL_BACK = 0x00002000
GM_FOG = 0x00010000
GM_LIGHTING = 0x00020000
GM_TEXTURE_GEN = 0x00040000
GM_TEXTURE_GEN_LINEAR = 0x00080000

# Other-mode H shifts (gbi.h G_MDSFT_*)
MDSFT_TEXTFILT = 12
MDSFT_TEXTLOD = 16
MDSFT_CYCLETYPE = 20
G_CYC_2CYCLE = 1 << MDSFT_CYCLETYPE
G_TF_POINT = 0

# Other-mode L render-mode bits (gbi.h)
AA_EN, Z_CMP, Z_UPD, IM_RD = 0x8, 0x10, 0x20, 0x40
CLR_ON_CVG = 0x80
ZMODE_MASK, ZMODE_DEC = 0xC00, 0xC00
CVG_X_ALPHA, ALPHA_CVG_SEL, FORCE_BL = 0x1000, 0x2000, 0x4000
G_AC_NONE, G_AC_THRESHOLD, G_AC_DITHER = 0, 1, 3

# The default geometry mode when `bgun_render` starts drawing: zbuf_configure_rdp
# sets G_ZBUFFER (`zbuf.c:258`), the chr/prop passes leave G_CULL_BACK set
# (`chr.c:6543`), and every model path uses smooth Gouraud (`bg.c:927`).
DEFAULT_GEOM = GM_ZBUFFER | GM_SHADE | GM_SHADING_SMOOTH | GM_CULL_BACK

# G_CC_TRILERP, G_CC_MODULATEIA2 (gbi.h) as fast3d's `color_comb` quadruples:
#   TRILERP     = TEXEL1, TEXEL0, LOD_FRACTION, TEXEL0 | same for alpha
#   MODULATEIA2 = COMBINED, 0, SHADE, 0                | COMBINED, 0, SHADE, 0
# Encoded straight into the SETCOMBINE words so the same decoder handles both.
def _cc_words(a0, b0, c0, d0, Aa0, Ab0, Ac0, Ad0, a1, b1, c1, d1, Aa1, Ab1, Ac1, Ad1):
    w0 = (G_SETCOMBINE << 24) | (a0 << 20) | (c0 << 15) | (Aa0 << 12) | (Ac0 << 9) | (a1 << 5) | c1
    w1 = (b0 << 28) | (b1 << 24) | (Aa1 << 21) | (Ac1 << 18) | (d0 << 15) | (Ab0 << 12) | (Ad0 << 9) | (d1 << 6) | (Ab1 << 3) | Ad1
    return w0 & 0xFFFFFFFF, w1 & 0xFFFFFFFF


# Colour mux ids: 0 COMBINED 1 TEXEL0 2 TEXEL1 3 PRIM 4 SHADE 5 ENV 6 ONE 7 NOISE
# (b: 6 CENTER, 7 K4; c: 6 SCALE, 7 COMBINED_A, 8 T0_A, 9 T1_A, 10 PRIM_A,
#  11 SHADE_A, 12 ENV_A, 13 LOD_FRAC, 14 PRIM_LOD_FRAC, 15 K5; d: 6 ONE 7 ZERO)
# Alpha mux ids: 0 COMBINED 1 TEXEL0 2 TEXEL1 3 PRIM 4 SHADE 5 ENV 6 ONE 7 ZERO
# (c: 0 LOD_FRACTION, 6 PRIM_LOD_FRAC)
CC_TRILERP_MODULATEIA2 = _cc_words(
    2, 1, 13, 1, 2, 1, 0, 1,     # cycle 1: TRILERP (alpha c=0 is LOD_FRACTION)
    0, 15, 4, 7, 0, 7, 4, 7,     # cycle 2: MODULATEIA2 (b=15 -> 0, d=7 -> 0)
)

# G_RM_FOG_PRIM_A (cycle 1) | G_RM_AA_ZB_OPA_SURF2 (cycle 2), resolved to the
# 32-bit other-mode-L word (render mode lives in bits 3..31).
#   FOG_PRIM_A  = GBL_c1(CLR_FOG, A_FOG, CLR_IN, 1MA)
#   AA_ZB_OPA_SURF2 = AA_EN|Z_CMP|Z_UPD|IM_RD|CVG_DST_CLAMP|ZMODE_OPA|ALPHA_CVG_SEL
#                     | GBL_c2(CLR_IN, A_IN, CLR_MEM, A_MEM)
def _gbl_c1(p, a, m, b):
    return (p << 30) | (a << 26) | (m << 22) | (b << 18)


def _gbl_c2(p, a, m, b):
    return (p << 28) | (a << 24) | (m << 20) | (b << 16)


BL_CLR_IN, BL_CLR_MEM, BL_CLR_BL, BL_CLR_FOG = 0, 1, 2, 3
BL_A_IN, BL_A_FOG, BL_A_SHADE, BL_0 = 0, 1, 2, 3
BL_1MA, BL_A_MEM, BL_1, BL_0B = 0, 1, 2, 3

RM_FOG_PRIM_A = _gbl_c1(BL_CLR_FOG, BL_A_FOG, BL_CLR_IN, BL_1MA)
RM_AA_ZB_OPA_SURF2 = AA_EN | Z_CMP | Z_UPD | IM_RD | ALPHA_CVG_SEL | _gbl_c2(BL_CLR_IN, BL_A_IN, BL_CLR_MEM, BL_A_MEM)
# AA_ZB_XLU_SURF2 = AA_EN|Z_CMP|IM_RD|CVG_DST_WRAP|CLR_ON_CVG|FORCE_BL|ZMODE_XLU
#                   | GBL_c2(CLR_IN, A_IN, CLR_MEM, 1MA)
RM_AA_ZB_XLU_SURF2 = AA_EN | Z_CMP | IM_RD | 0x100 | CLR_ON_CVG | FORCE_BL | 0x800 | _gbl_c2(BL_CLR_IN, BL_A_IN, BL_CLR_MEM, BL_1MA)

OML_BONDGUN_OPA = RM_FOG_PRIM_A | RM_AA_ZB_OPA_SURF2
OML_BONDGUN_XLU = RM_FOG_PRIM_A | RM_AA_ZB_XLU_SURF2

# model_apply_rendermode_* also force G_CYC_2CYCLE in other-mode H.
MODELRENDERMODE_0 = 0
MODELRENDERMODE_SIMPLE = 1
MODELRENDERMODE_TRILERP = 2
MODELRENDERMODE_CTXAWARE_1PASS = 3
MODELRENDERMODE_CTXAWARE_2PASS = 4

# G_CC_MODULATEIA, G_CC_MODULATEIA (SIMPLE mode, model.c:2811)
CC_MODULATEIA = _cc_words(1, 15, 4, 7, 1, 7, 4, 7, 1, 15, 4, 7, 1, 7, 4, 7)
OML_AA_ZB_OPA_SURF = (AA_EN | Z_CMP | Z_UPD | IM_RD | ALPHA_CVG_SEL
                      | _gbl_c1(BL_CLR_IN, BL_A_IN, BL_CLR_MEM, BL_A_MEM)
                      | _gbl_c2(BL_CLR_IN, BL_A_IN, BL_CLR_MEM, BL_A_MEM))

NODE_POSITION, NODE_GUNDL, NODE_DISTANCE, NODE_REORDER = 0x02, 0x04, 0x08, 0x09
NODE_TYPE11, NODE_TOGGLE, NODE_POSITIONHELD, NODE_STARGUNFIRE, NODE_DL = 0x11, 0x12, 0x15, 0x16, 0x18
NODE_BBOX = 0x0A
NODE_CHRGUNFIRE = 0x0C
NODE_CHRINFO = 0x01

VTX_SIZE = 12


def decode_combine(w0: int, w1: int) -> list[int]:
    """fast3d's G_SETCOMBINE unpacking (`gfx_pc.cpp:2398`) as 16 mux ids:
    [a0,b0,c0,d0, Aa0,Ab0,Ac0,Ad0, a1,b1,c1,d1, Aa1,Ab1,Ac1,Ad1]."""
    def c0(s, n):
        return (w0 >> s) & ((1 << n) - 1)

    def c1(s, n):
        return (w1 >> s) & ((1 << n) - 1)

    return [
        c0(20, 4), c1(28, 4), c0(15, 5), c1(15, 3),
        c0(12, 3), c1(12, 3), c0(9, 3), c1(9, 3),
        c0(5, 4), c1(24, 4), c0(0, 5), c1(6, 3),
        c1(21, 3), c1(3, 3), c1(18, 3), c1(0, 3),
    ]


def blend_from_othermode(oml: int, two_cycle: bool) -> dict:
    """Map the RDP blender + z bits to what a modern pipeline needs.

    The cycle that writes the framebuffer is cycle 2 in two-cycle mode and cycle 1
    otherwise; in two-cycle mode cycle 1 is PD's fog/shade tint (`FOG_PRIM_A`,
    `envcolour` = gunshadecol), which the shader applies as a uniform mix.
    fast3d's rule (`gfx_pc.cpp:1292`): alpha blending iff M = CLR_MEM and B = 1MA,
    or the texture-edge coverage mode is on.
    """
    if two_cycle:
        p, a, m, b = (oml >> 28) & 3, (oml >> 24) & 3, (oml >> 20) & 3, (oml >> 16) & 3
    else:
        p, a, m, b = (oml >> 30) & 3, (oml >> 26) & 3, (oml >> 22) & 3, (oml >> 18) & 3
    zmode = oml & ZMODE_MASK
    tex_edge = bool(oml & CVG_X_ALPHA)
    ac = oml & 3
    # Only `M = CLR_MEM, B = 1MA` is real blending. `B = A_MEM` (the *_SURF2
    # modes) blends by framebuffer COVERAGE, which is only partial on polygon
    # edges — that is the N64's antialiasing, and interior pixels are opaque.
    blend = "alpha" if (m == BL_CLR_MEM and b == BL_1MA) else "opaque"
    alpha_test = "edge" if tex_edge else ("threshold" if ac == G_AC_THRESHOLD else "none")
    return {
        "blend": blend,
        "ztest": bool(oml & Z_CMP),
        "zwrite": bool(oml & Z_UPD),
        "decal": zmode == ZMODE_DEC,
        "alpha_test": alpha_test,
        "fog_tint": (((oml >> 30) & 3) == BL_CLR_FOG) if two_cycle else False,
    }


# ---------------------------------------------------------------------------
# Texture tiles — the C0 expansion (`tex_load_from_gdl`, tex.c:~850)
# ---------------------------------------------------------------------------

TXMODE_WRAP, TXMODE_CLAMP, TXMODE_MIRROR = 0, 1, 2


def pool_hasloddata(texnum: int) -> bool | None:
    """Byte 0 bit 7 of `textures/NNNN.bin` (`texdecompress.c`), readable even for
    the codec we cannot decode."""
    path = os.path.join(ASSETS, "textures", f"{texnum:04x}.bin")
    try:
        with open(path, "rb") as fh:
            b = fh.read(1)
        return bool(b[0] & 0x80) if b else None
    except OSError:
        return None


class Tile:
    """Render tile 0 as PD's C0 expansion (or inline SETTILE) leaves it."""

    def __init__(self) -> None:
        self.texkey: int | None = None  # key into read_texconfigs
        self.cms = TXMODE_WRAP
        self.cmt = TXMODE_WRAP
        self.shifts = 0
        self.shiftt = 0
        self.uls = 0.0  # in texels
        self.ult = 0.0
        self.mipmapped = False

    def snapshot(self) -> tuple:
        return (self.texkey, self.cms, self.cmt, self.shifts, self.shiftt, self.uls, self.ult, self.mipmapped)


def gbi_cm_to_txmode(cm: int) -> int:
    # gbi: G_TX_MIRROR = 1, G_TX_CLAMP = 2
    if cm & 2:
        return TXMODE_CLAMP if not (cm & 1) else TXMODE_CLAMP
    if cm & 1:
        return TXMODE_MIRROR
    return TXMODE_WRAP


# ---------------------------------------------------------------------------
# The interpreter
# ---------------------------------------------------------------------------


def cull_name(geom: int) -> str:
    if geom & GM_CULL_BACK and geom & GM_CULL_FRONT:
        return "both"
    if geom & GM_CULL_BACK:
        return "back"
    if geom & GM_CULL_FRONT:
        return "front"
    return "none"


class RspState:
    def __init__(self) -> None:
        self.geom = DEFAULT_GEOM
        self.omh = G_CYC_2CYCLE | (1 << MDSFT_TEXTLOD) | (2 << MDSFT_TEXTFILT)
        self.oml = OML_BONDGUN_OPA
        self.combine = CC_TRILERP_MODULATEIA2
        self.tex_s = 0xFFFF
        self.tex_t = 0xFFFF
        self.tex_on = True
        self.tile = Tile()
        self.prim = [255, 255, 255, 255]
        self.env = None  # None -> renderdata envcolour
        self.fog = None  # None -> renderdata fog (gunshadecol)
        self.cur_mtx = -1
        self.colours_off: int | None = None  # file offset of the G_COL table
        self.colours_count = 0
        #: Whether the node being interpreted has itself set/cleared cull bits.
        #: Cull is the one bit of state that genuinely leaks between nodes in the
        #: shipped guns (the flash node clears it and nothing sets it back), and
        #: which nodes precede a batch depends on runtime part visibility — so a
        #: batch whose node never touched cull says "inherit" and Rust threads the
        #: state through the nodes actually drawn that frame.
        self.cull_touched = False


class Interp:
    """Walk one model's drawable nodes in `model_render` order."""

    def __init__(self, m: pd_model.ModelDef, texconfigs: dict, name: str):
        self.m = m
        self.d = m.data
        self.name = name
        self.cfgs = texconfigs
        self.st = RspState()
        self.materials: list[dict] = []
        self.material_index: dict[str, int] = {}
        self.batches: list[dict] = []  # {node, material, verts: [...], indices: [...]}
        self.used_textures: set[int] = set()
        self.warnings: list[str] = []

    # -- helpers ----------------------------------------------------------

    def resolve(self, addr: int, segs: dict[int, int]) -> int | None:
        base = segs.get(addr >> 24)
        if base is None:
            return None
        off = base + (addr & 0xFFFFFF)
        return off if 0 <= off < len(self.d) else None

    def material_key(self, lit: bool, texgen: bool) -> int:
        st = self.st
        two = (st.omh & (3 << MDSFT_CYCLETYPE)) == G_CYC_2CYCLE
        cull = cull_name(st.geom) if st.cull_touched else "inherit"
        mux = decode_combine(*st.combine)
        tile = st.tile
        texkey = tile.texkey if st.tex_on else None
        tex = None
        if texkey is not None:
            cfg = self.cfgs.get(texkey)
            if cfg is not None:
                texid = cfg.texnum if cfg.texnum is not None else (0x10000 | cfg.index)
                tex = {
                    "id": texid,
                    "cms": tile.cms,
                    "cmt": tile.cmt,
                    "shifts": tile.shifts,
                    "shiftt": tile.shiftt,
                    "uls": tile.uls,
                    "ult": tile.ult,
                    "mipmap": tile.mipmapped,
                    "linear": ((st.omh >> MDSFT_TEXTFILT) & 3) != G_TF_POINT,
                }
                self.used_textures.add(texkey)
            else:
                self.warnings.append(f"texture key {texkey:#x} not in texconfigs")
        mat = {
            "two_cycle": two,
            "combine": mux,
            "cull": cull,
            "lighting": lit,
            "texgen": texgen,
            "texgen_linear": bool(st.geom & GM_TEXTURE_GEN_LINEAR),
            "texture": tex,
            "prim": list(st.prim),
            "env": st.env,
            "fog": st.fog,
            **blend_from_othermode(st.oml, two),
        }
        key = json.dumps(mat, sort_keys=True)
        idx = self.material_index.get(key)
        if idx is None:
            idx = len(self.materials)
            self.material_index[key] = idx
            self.materials.append(mat)
        return idx

    # -- execution ----------------------------------------------------------

    def run_node(self, node_index: int, node: pd_model.Node, rendermode: int, gdls: list[tuple[int, bool]],
                 segs: dict[int, int], fallback_mtx: int) -> str | None:
        """Interpret one drawable node. Returns the cull state it leaves behind if
        it changed it (see `RspState.cull_touched`), else None."""
        batch_cur: dict | None = None
        vbuf: list[dict | None] = [None] * 16
        self.st.cull_touched = False

        def emit_tri(ia: int, ib: int, ic: int) -> None:
            nonlocal batch_cur
            verts = [vbuf[ia], vbuf[ib], vbuf[ic]]
            if any(v is None for v in verts):
                return
            # A triangle's shading is decided per vertex at load time (lit/texgen),
            # but PD never mixes them inside one draw, so the first vertex decides.
            mat = self.material_key(verts[0]["lit"], verts[0]["texgen"])
            if batch_cur is None or batch_cur["material"] != mat:
                batch_cur = {"node": node_index, "material": mat, "verts": [], "remap": {}, "indices": []}
                self.batches.append(batch_cur)
            for v in verts:
                k = v["uid"]
                vi = batch_cur["remap"].get(k)
                if vi is None:
                    vi = len(batch_cur["verts"])
                    batch_cur["remap"][k] = vi
                    batch_cur["verts"].append(v)
                batch_cur["indices"].append(vi)

        uid_counter = [0]

        def run(addr: int, depth: int) -> None:
            nonlocal batch_cur
            off = self.resolve(addr, segs)
            if off is None or depth > 8:
                return
            st = self.st
            while off + 8 <= len(self.d):
                w0, w1 = struct.unpack_from(">II", self.d, off)
                off += 8
                op = w0 >> 24
                if op == G_ENDDL:
                    return
                if op == G_MTX:
                    if (w1 >> 24) == 0x03:
                        st.cur_mtx = (w1 & 0xFFFFFF) // 64
                    continue
                if op == G_COL:
                    o = self.resolve(w1, segs)
                    st.colours_off = o
                    st.colours_count = (w0 & 0xFFFF) // 4
                    continue
                if op == G_VTX:
                    n = (w0 & 0xFFFF) // VTX_SIZE
                    dest = (w0 >> 16) & 0xF
                    src = self.resolve(w1, segs)
                    if src is None:
                        continue
                    for i in range(n):
                        vo = src + i * VTX_SIZE
                        if vo + VTX_SIZE > len(self.d) or dest + i >= 16:
                            break
                        x, y, z, flags, colour, s, t = struct.unpack_from(">hhhBBhh", self.d, vo)
                        ci = colour >> 2
                        if st.colours_off is not None:
                            co = st.colours_off + ci * 4
                        else:
                            co = segs[6] + ci * 4
                        cbytes = list(self.d[co : co + 4]) if co + 4 <= len(self.d) else [255, 255, 255, 255]
                        lit = bool(st.geom & GM_LIGHTING)
                        texgen = lit and bool(st.geom & GM_TEXTURE_GEN)
                        # fast3d gfx_sp_vertex: U = s * scale >> 16, in S10.5 texels.
                        if texgen:
                            u, v = st.tex_s / 32.0, st.tex_t / 32.0  # texgen: scale, see shader
                        else:
                            U = (s * st.tex_s) >> 16
                            V = (t * st.tex_t) >> 16
                            # `short U = ...` in fast3d: wrap to s16.
                            U = ((U + 0x8000) & 0xFFFF) - 0x8000
                            V = ((V + 0x8000) & 0xFFFF) - 0x8000
                            u, v = U / 32.0, V / 32.0
                        uid_counter[0] += 1
                        vbuf[dest + i] = {
                            "uid": (node_index, uid_counter[0]),
                            "pos": (x, y, z),
                            "mtx": st.cur_mtx if st.cur_mtx >= 0 else fallback_mtx,
                            "uv": (u, v),
                            "c": cbytes,
                            "lit": lit,
                            "texgen": texgen,
                        }
                    continue
                if op == G_TRI4:
                    zs = [(w0 >> (4 * i)) & 0xF for i in range(4)]
                    xs = [(w1 >> (8 * i)) & 0xF for i in range(4)]
                    ys = [(w1 >> (8 * i + 4)) & 0xF for i in range(4)]
                    for i in range(4):
                        if xs[i] or ys[i] or zs[i]:
                            emit_tri(xs[i], ys[i], zs[i])
                    continue
                if op == G_TRI1:
                    emit_tri(((w1 >> 16) & 0xFF) // 10, ((w1 >> 8) & 0xFF) // 10, (w1 & 0xFF) // 10)
                    continue
                if op == G_SETGEOM:
                    st.geom |= w1
                    if w1 & (GM_CULL_BACK | GM_CULL_FRONT):
                        st.cull_touched = True
                    batch_cur = None
                    continue
                if op == G_CLEARGEOM:
                    st.geom &= ~w1
                    if w1 & (GM_CULL_BACK | GM_CULL_FRONT):
                        st.cull_touched = True
                    batch_cur = None
                    continue
                if op == G_SETOTHERMODE_H or op == G_SETOTHERMODE_L:
                    sft = (w0 >> 8) & 0xFF
                    ln = w0 & 0xFF
                    mask = ((1 << ln) - 1) << sft
                    if op == G_SETOTHERMODE_H:
                        st.omh = (st.omh & ~mask) | (w1 & mask)
                    else:
                        st.oml = (st.oml & ~mask) | (w1 & mask)
                    batch_cur = None
                    continue
                if op == G_SETCOMBINE:
                    st.combine = (w0, w1)
                    batch_cur = None
                    continue
                if op == G_TEXTURE:
                    st.tex_s = (w1 >> 16) & 0xFFFF
                    st.tex_t = w1 & 0xFFFF
                    st.tex_on = bool(w0 & 0xFF)
                    batch_cur = None
                    continue
                if op == G_SETPRIMCOLOR:
                    st.prim = [(w1 >> 24) & 0xFF, (w1 >> 16) & 0xFF, (w1 >> 8) & 0xFF, w1 & 0xFF]
                    batch_cur = None
                    continue
                if op == G_SETENVCOLOR:
                    st.env = [(w1 >> 24) & 0xFF, (w1 >> 16) & 0xFF, (w1 >> 8) & 0xFF, w1 & 0xFF]
                    batch_cur = None
                    continue
                if op == G_SETFOGCOLOR:
                    st.fog = [(w1 >> 24) & 0xFF, (w1 >> 16) & 0xFF, (w1 >> 8) & 0xFF, w1 & 0xFF]
                    batch_cur = None
                    continue
                if op == G_SETTEXNUM:
                    self.c0(w0, w1)
                    batch_cur = None
                    continue
                if op == G_SETTIMG:
                    st.tile.texkey = w1
                    st.tile.mipmapped = False
                    batch_cur = None
                    continue
                if op == G_SETTILE:
                    tile = (w1 >> 24) & 7
                    if tile == 0:
                        st.tile.cmt = gbi_cm_to_txmode((w1 >> 18) & 3)
                        st.tile.shiftt = (w1 >> 10) & 0xF
                        st.tile.cms = gbi_cm_to_txmode((w1 >> 8) & 3)
                        st.tile.shifts = w1 & 0xF
                    batch_cur = None
                    continue
                if op == G_SETTILESIZE:
                    tile = (w1 >> 24) & 7
                    if tile == 0:
                        st.tile.uls = ((w0 >> 12) & 0xFFF) / 4.0
                        st.tile.ult = (w0 & 0xFFF) / 4.0
                    continue
                if op == G_DL:
                    if (w0 >> 16) & 0xFF:
                        nxt = self.resolve(w1, segs)
                        if nxt is None:
                            return
                        off = nxt
                        continue
                    run(w1, depth + 1)
                    continue
                # Everything else (syncs, LOADBLOCK/TLUT, MOVEWORD, ...) doesn't
                # change what we need.

        # model_render_node_gundl: the context setup, then opa, then (1PASS) xlu.
        st = self.st
        for gdl, is_xlu in gdls:
            if rendermode in (MODELRENDERMODE_CTXAWARE_1PASS, MODELRENDERMODE_CTXAWARE_2PASS):
                # model_apply_rendermode_ctxaware_*pass, MODELRENDERCONTEXT_BONDGUN_OPA
                st.omh = (st.omh & ~(3 << MDSFT_CYCLETYPE)) | G_CYC_2CYCLE
                st.combine = CC_TRILERP_MODULATEIA2
                st.oml = OML_BONDGUN_XLU if is_xlu else OML_BONDGUN_OPA
                st.fog = None
            elif rendermode == MODELRENDERMODE_SIMPLE:
                st.omh = st.omh & ~(3 << MDSFT_CYCLETYPE)
                st.oml = OML_AA_ZB_OPA_SURF
                st.combine = CC_MODULATEIA
            elif rendermode == MODELRENDERMODE_TRILERP:
                st.omh = (st.omh & ~(3 << MDSFT_CYCLETYPE)) | G_CYC_2CYCLE
                st.oml = RM_AA_ZB_OPA_SURF2  # G_RM_PASS in cycle 1
                st.combine = CC_TRILERP_MODULATEIA2
            batch_cur = None
            run(gdl, 0)
        return cull_name(st.geom) if st.cull_touched else None

    def c0(self, w0: int, w1: int) -> None:
        """`tex_load_from_gdl`'s C0 expansion (tex.c) for render tile 0."""
        st = self.st
        texnum = w1 & 0xFFF
        subcmd = w0 & 7
        smode = (w0 >> 22) & 3
        tmode = (w0 >> 20) & 3
        offset = (w0 >> 18) & 3
        t = st.tile
        t.texkey = texnum
        t.cms = smode
        t.cmt = tmode
        hasloddata = pool_hasloddata(texnum)
        half = 0.5 if (offset == 2 and not hasloddata) else 0.0
        if subcmd in (0, 1):
            # type0/1: tile 0 comes from `tex_write_tile_from_definition` — always
            # WRAP, with the command's own shifts, and `min` goes to prim's LOD frac.
            t.cms = TXMODE_WRAP
            t.cmt = TXMODE_WRAP
            t.shifts = (w0 >> 14) & 0xF
            t.shiftt = (w0 >> 10) & 0xF
            st.prim = [255, 255, 255, 255]
            t.mipmapped = False
        elif subcmd == 2:
            t.shifts = 0
            t.shiftt = 0
            t.mipmapped = True
        else:  # 3, 4: single-LOD tile(s)
            t.shifts = 0
            t.shiftt = 0
            t.mipmapped = False
        t.uls = half
        t.ult = half


def walk_with_parents(m: pd_model.ModelDef) -> tuple[list[pd_model.Node], dict[int, int]]:
    nodes = m.walk()
    index = {n.offset: i for i, n in enumerate(nodes)}
    return nodes, index


def model_parts(m: pd_model.ModelDef) -> dict[int, int]:
    return pd_gltf.model_parts(m)


def export_model(path: str, texdir: str | None, texprefix: str = "tex_", tex_sink=None) -> tuple[dict, list[str]]:
    """One model file → the exporter's dict (nodes, parts, materials, batches, textures).

    Textures go to `tex_sink(model, texconfig, texid, stem) -> entry` when given
    (pd_models.py's global pool), else they are written into `texdir` as before.
    The sink raises `pd_tex.UnsupportedTexture` for a texture it cannot decode."""
    m = pd_model.load(path)
    cfgs = pd_gltf.read_texconfigs(m)
    nodes, index = walk_with_parents(m)
    parts = model_parts(m)
    part_of_node = {off: p for p, off in parts.items()}
    interp = Interp(m, cfgs, m.name)

    out_nodes: list[dict] = []
    # nearest ancestor POSITION's matrix, for vertices with no G_MTX (none in the
    # shipped guns, but the fallback must be a real slot).
    def ancestor_mtx(n: pd_model.Node) -> int:
        cur = n
        while cur is not None:
            if (cur.type & 0xFF) == NODE_POSITION and seg_ok(cur.rodata):
                _, _, _, _, i0, _, _ = struct.unpack_from(">fffHhhh", m.data, seg_off(cur.rodata))
                return i0
            if (cur.type & 0xFF) == NODE_POSITIONHELD and seg_ok(cur.rodata):
                return struct.unpack_from(">fffh", m.data, seg_off(cur.rodata))[3]
            cur = m.read_node(seg_off(cur.parent)) if seg_ok(cur.parent) else None
        return 0

    for i, n in enumerate(nodes):
        t = n.type & 0xFF
        parent = index.get(seg_off(n.parent), -1) if seg_ok(n.parent) else -1
        rec: dict = {"type": n.typename.lower(), "parent": parent}
        if n.offset in part_of_node:
            rec["partnum"] = part_of_node[n.offset]
        ro = seg_off(n.rodata) if seg_ok(n.rodata) else None
        if t == NODE_CHRINFO and ro is not None:
            # struct modelrodata_chrinfo (types.h): the chr root that
            # model_update_chr_node_mtx (model.c:726) positions.
            animpart, mtxindex = struct.unpack_from(">Hh", m.data, ro)
            rec.update(animpart=animpart, mtx=[mtxindex, -1, -1])
        elif t == NODE_POSITION and ro is not None:
            x, y, z, part, i0, i1, i2 = struct.unpack_from(">fffHhhh", m.data, ro)
            rec.update(pos=[x, y, z], animpart=part, mtx=[i0, i1, i2], flags=n.type & 0xFF00)
        elif t == NODE_POSITIONHELD and ro is not None:
            x, y, z, mi = struct.unpack_from(">fffh", m.data, ro)
            rec.update(pos=[x, y, z], mtx=[mi, -1, -1])
        elif t == NODE_TOGGLE and ro is not None:
            tgt, rw = struct.unpack_from(">IH", m.data, ro)
            rec.update(rw=rw, target=index.get(seg_off(tgt), -1) if seg_ok(tgt) else -1)
        elif t == NODE_DISTANCE and ro is not None:
            near, far, tgt, rw = struct.unpack_from(">ffIH", m.data, ro)
            rec.update(near=near, far=far, rw=rw, target=index.get(seg_off(tgt), -1) if seg_ok(tgt) else -1)
        elif t == NODE_CHRGUNFIRE and ro is not None:
            # struct modelrodata_chrgunfire (types.h:502): a billboarded muzzle
            # flash drawn by model_render_node_chr_gunfire (model.c:3368).
            x, y, z, dx, dy, dz, texptr = struct.unpack_from(">ffffffI", m.data, ro)
            rec.update(pos=[x, y, z], dim=[dx, dy, dz])
            if seg_ok(texptr) and seg_ok(m.texconfigs):
                cfgindex = (seg_off(texptr) - seg_off(m.texconfigs)) // pd_gltf.TEXCONFIG_SIZE
                for key, cfg in cfgs.items():
                    if cfg.index == cfgindex:
                        interp.used_textures.add(key)
                        rec["texture"] = cfg.texnum if cfg.texnum is not None else (0x10000 | cfg.index)
                        rec["texture_size"] = [cfg.width, cfg.height]
        elif t == NODE_BBOX and ro is not None:
            # struct modelrodata_bbox (types.h:470): what obj_find_bbox_rodata
            # hands the projectile/settle maths (propobj.c).
            hitpart, x0, x1, y0, y1, z0, z1 = struct.unpack_from(">iffffff", m.data, ro)
            rec.update(hitpart=hitpart, bbox=[x0, x1, y0, y1, z0, z1])
        elif t in (NODE_GUNDL, NODE_DL) and ro is not None:
            if t == NODE_GUNDL:
                opa, xlu, base, vtx, nv, rm = struct.unpack_from(">IIIIhh", m.data, ro)
                col_off = None
            else:
                opa, xlu, col, vtx, nv, rm = struct.unpack_from(">IIIIhh", m.data, ro)
                base = 0
                col_off = seg_off(col) if seg_ok(col) else None
            vb = seg_off(vtx)
            segs = {0x04: vb, 0x05: (seg_off(base) if seg_ok(base) else 0),
                    0x06: col_off if col_off is not None else vb + max(nv, 0) * VTX_SIZE}
            gdls = []
            if opa:
                gdls.append((opa, False))
            if xlu and rm == MODELRENDERMODE_CTXAWARE_1PASS:
                gdls.append((xlu, True))
            elif xlu and rm == MODELRENDERMODE_CTXAWARE_2PASS:
                gdls.append((xlu, True))
            interp.st.colours_off = None
            cull_exit = interp.run_node(i, n, rm, gdls, segs, ancestor_mtx(n))
            rec.update(rendermode=rm)
            if cull_exit is not None:
                rec["cull_exit"] = cull_exit
        elif t == NODE_STARGUNFIRE and ro is not None:
            count, vtx, gdl, base = struct.unpack_from(">iIII", m.data, ro)
            vb = seg_off(vtx)
            segs = {0x04: vb, 0x05: seg_off(base) if seg_ok(base) else 0, 0x06: vb + count * 4 * VTX_SIZE}
            # Run the DL once to capture its state + triangles (the per-frame
            # jitter of `model_render_node_star_gunfire` is replayed in Rust).
            interp.st.colours_off = None
            first_batch = len(interp.batches)
            cull_exit = interp.run_node(i, n, MODELRENDERMODE_0, [(gdl, True)], segs, ancestor_mtx(n))
            if cull_exit is not None:
                rec["cull_exit"] = cull_exit
            quads = []
            for q in range(count):
                qv = []
                for k in range(4):
                    x, y, z, _f, _c, s, tt = struct.unpack_from(">hhhBBhh", m.data, vb + (q * 4 + k) * VTX_SIZE)
                    qv.append([x, y, z])
                quads.append(qv)
            rec.update(quads=quads, batches=list(range(first_batch, len(interp.batches))))
        out_nodes.append(rec)

    # Textures actually drawn.
    textures: dict[str, dict] = {}
    for texkey in sorted(interp.used_textures):
        cfg = cfgs[texkey]
        texid = cfg.texnum if cfg.texnum is not None else (0x10000 | cfg.index)
        # Embedded textures are numbered per file, so their PNGs carry the model's
        # name (two casings both have an embedded texture 0).
        stem = os.path.splitext(os.path.basename(path))[0]
        if tex_sink is not None:
            try:
                textures[str(texid)] = tex_sink(m, cfg, texid, stem)
            except pd_tex.UnsupportedTexture as e:
                interp.warnings.append(f"texture {texid:#x} undecodable: {e}")
            continue
        fname = f"{texprefix}{stem}_{texid & 0xFFFF:03x}.png" if texid >= 0x10000 else f"{texprefix}{texid:04x}.png"
        try:
            w, h, rgba, src = pd_gltf.resolve_texture(m, cfg, cfg.texnum if cfg.texnum is not None else -1)
        except (pd_tex.UnsupportedTexture, SystemExit) as e:
            interp.warnings.append(f"texture {texid:#x} undecodable: {e}")
            continue
        with open(os.path.join(texdir, fname), "wb") as fh:
            fh.write(pd_gltf.png_bytes(w, h, rgba))
        textures[str(texid)] = {"file": fname, "w": w, "h": h, "cfg_w": cfg.width, "cfg_h": cfg.height,
                                "levels": cfg.levels, "source": src}

    # Flatten batches.
    batches = []
    for b in interp.batches:
        verts = []
        for v in b["verts"]:
            flags = (1 if v["lit"] else 0) | (2 if v["texgen"] else 0)
            verts.append([v["pos"][0], v["pos"][1], v["pos"][2], v["mtx"],
                          round(v["uv"][0], 5), round(v["uv"][1], 5),
                          v["c"][0], v["c"][1], v["c"][2], v["c"][3], flags])
        batches.append({"node": b["node"], "material": b["material"], "verts": verts, "indices": b["indices"]})

    return {
        "name": m.name,
        "source": decomp_rel(path),
        "nummatrices": m.nummatrices,
        "skel": m.skel,
        "nodes": out_nodes,
        "parts": {str(p): index[off] for p, off in parts.items() if off in index},
        "materials": interp.materials,
        "batches": batches,
        "textures": textures,
        "vertex_layout": ["x", "y", "z", "mtx", "u", "v", "c0", "c1", "c2", "c3", "flags(1=lit,2=texgen)"],
    }, interp.warnings


# ---------------------------------------------------------------------------
# Weapon tables (+ what pd_weapons.py skips)
# ---------------------------------------------------------------------------

GUNCMD_RE = re.compile(r"^\s*struct\s+guncmd\s+(\w+)\s*\[\s*\]\s*=\s*\{", re.M)
GUNSCRIPT_RE = re.compile(r"gunscript_(\w+)\s*\(([^)]*)\)|gunscript_end|\{\s*(GUNCMD_\w+)\s*,([^}]*)\}")

AIM_FIELDS = ["zoomfov", "guntransup", "guntransdown", "guntransside", "aimdamppal", "aimdamp",
              "tracktype", "unused", "flags"]
RECOIL_FIELDS = ["xrange", "yrange", "zrange", "unk0c", "unk10"]
NOISE_FIELDS = ["minradius", "maxradius", "incradius", "decbasespeed", "decremspeed"]
GUNVIS_FIELDS = ["type", "param", "op", "partnum", "unk"]


def load_anim_table() -> tuple[list[dict], dict[str, int]]:
    with open(os.path.join(ASSETS, "animations.json"), encoding="utf-8") as fh:
        table = json.load(fh)
    return table, {a["id"]: i for i, a in enumerate(table)}


def scrape_sfx() -> dict[str, int]:
    """`enum sfxnum` + `enum sfxmap` (include/sfx.h:27, :1580 — the mapped ids
    start at an explicit 0x8000)."""
    p = src("include", "sfx.h")
    return pd_weapons.scrape_enums(p, ("sfxnum", "sfxmap"))


def parse_guncmds(text: str, consts: pd_weapons.Consts, anims: dict[str, int], sfx: dict[str, int]) -> dict[str, list]:
    """Every `struct guncmd X[]` as a list of decoded commands (gunscript.h)."""
    out: dict[str, list] = {}

    def val(tok: str):
        tok = tok.strip()
        if tok in anims:
            return anims[tok]
        if tok in sfx:
            return sfx[tok]
        if tok.startswith("ANIM_") and re.fullmatch(r"ANIM_[0-9A-Fa-f]{4}", tok):
            return int(tok[5:], 16)
        v = pd_weapons.resolve_expr(tok, consts)
        return v

    for mm in GUNCMD_RE.finditer(text):
        name = mm.group(1)
        i = mm.end() - 1
        depth = 0
        while i < len(text):
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        body = text[mm.end(): i]
        line = text.count("\n", 0, mm.start()) + 1
        cmds = []
        for cm in re.finditer(r"gunscript_(\w+)\s*\(([^)]*)\)|gunscript_end\b|\{\s*(GUNCMD_\w+)\s*,([^}]*)\}", body):
            if cm.group(0).startswith("gunscript_end"):
                cmds.append({"op": "end"})
                continue
            if cm.group(3):
                # a raw `{ GUNCMD_X, cond, a, b }` initialiser
                args = [a.strip() for a in cm.group(4).split(",")]
                op = cm.group(3)[len("GUNCMD_"):].lower()
                rec = {"op": op, "raw": [val(a) for a in args]}
                if op == "playanimation":
                    rec = {"op": "playanimation", "condition": val(args[0]), "anim": val(args[1]),
                           "params": val(args[2]) if len(args) > 2 else 10000}
                cmds.append(rec)
                continue
            op = cm.group(1)
            args = [a.strip() for a in cm.group(2).split(",")] if cm.group(2).strip() else []
            if op == "playanimation":
                anim, direction, speed = val(args[0]), val(args[1]), val(args[2])
                # gunscript.h: `(direction << 16) | speed` stored in an s32 — the two
                # reverse scripts pass (65535, 55536), which is -10000: speed -1.0.
                params = ((int(direction) << 16) | int(speed)) & 0xFFFFFFFF
                if params & 0x80000000:
                    params -= 0x100000000
                cmds.append({"op": op, "anim": anim, "params": params})
            elif op in ("showpart", "hidepart"):
                cmds.append({"op": op, "keyframe": val(args[0]), "part": val(args[1])})
            elif op == "waitforzreleased":
                cmds.append({"op": op, "keyframe": val(args[0])})
            elif op == "allowfeature":
                cmds.append({"op": op, "keyframe": val(args[0]), "feature": val(args[1])})
            elif op == "playsound":
                cmds.append({"op": op, "keyframe": val(args[0]), "sound": val(args[1]), "sound_name": args[1]})
            elif op == "include":
                cmds.append({"op": op, "condition": val(args[0]), "target": args[1]})
            elif op == "random":
                cmds.append({"op": op, "probability": val(args[0]), "target": args[1]})
            elif op == "repeatuntilfull":
                cmds.append({"op": op, "keyframe": val(args[0]), "gotokeyframe": val(args[1])})
            elif op == "popoutsackofpills":
                cmds.append({"op": op, "keyframe": val(args[0])})
            elif op == "setsoundspeed":
                cmds.append({"op": op, "keyframe": val(args[0]), "speed": val(args[1])})
            else:
                cmds.append({"op": op, "args": [val(a) for a in args]})
        out[name] = {"line": line, "cmds": cmds}
    return out


def build_weapons() -> dict:
    """pd_weapons.build() + scripts/aim/recoil/noise + every referenced anim id."""
    table = pd_weapons.build(include_unarmed=True)
    consts = pd_weapons.Consts()
    consts.all.update(pd_weapons.scrape_defines(
        pd_weapons.src("include", "constants.h"),
        ("INVAIMFLAG_", "SIGHTTRACKTYPE_", "HANDATTACKTYPE_", "AMMOFLAG_"),
    ))
    raw = pd_weapons.resolve_version_ifs(pd_weapons.strip_comments(
        open(pd_weapons.src("game", "invitems.c"), encoding="utf-8", errors="replace").read()))
    inits = pd_weapons.find_initializers(raw)
    anim_table, anims = load_anim_table()
    sfx = scrape_sfx()
    scripts = parse_guncmds(raw, consts, anims, sfx)

    def struct_of(kind: str, fields: list[str]) -> dict:
        out = {}
        for name, info in inits.items():
            if info["struct"] == kind and not info["array"]:
                out[name] = {"line": info["line"], **pd_weapons.map_fields(info["body"], fields, consts)}
        return out

    aims = struct_of("invaimsettings", AIM_FIELDS)
    recoils = struct_of("recoilsettings", RECOIL_FIELDS)
    noises = struct_of("noisesettings", NOISE_FIELDS)

    # gunviscmds arrays: gunviscmd_* macros.
    gunvis: dict[str, list] = {}
    for mm in re.finditer(r"struct\s+gunviscmd\s+(\w+)\s*\[\s*\]\s*=\s*\{(.*?)\};", raw, re.S):
        cmds = []
        for cm in re.finditer(r"gunviscmd_(\w+)\s*\(([^)]*)\)", mm.group(2)):
            args = [pd_weapons.resolve_expr(a, consts) for a in cm.group(2).split(",")]
            cmds.append({"op": cm.group(1), "args": args})
        gunvis[mm.group(1)] = cmds

    # Pull the raw weapondef for fields pd_weapons drops (pritosec/sectopri names).
    wdefs = {}
    for name, info in inits.items():
        if info["struct"] == "weapondef" and not info["array"]:
            fields = pd_weapons.weapondef_fields(info["body"])
            wdefs[name] = pd_weapons.map_fields(info["body"], fields, consts)

    used_scripts: set[str] = set()

    def closure(name):
        if not name or name in used_scripts or name not in scripts:
            return
        used_scripts.add(name)
        for c in scripts[name]["cmds"]:
            if c["op"] in ("include", "random"):
                closure(c["target"])

    def reresolve(v):
        # pd_weapons.build() resolved constants before the extra prefixes above
        # were scraped, so AMMOFLAG_* etc. can still be source text.
        if isinstance(v, str):
            r = pd_weapons.resolve_expr(v, consts)
            return r if not isinstance(r, str) else 0
        return v

    for w in table["weapons"]:
        for a in w.get("ammo") or []:
            if a:
                a["flags"] = reresolve(a.get("flags"))
        for f in w.get("functions") or []:
            if f:
                f["flags"] = reresolve(f.get("flags"))
        wd = wdefs.get(w["symbol"], {})
        w["equip_animation"] = wd.get("equip_animation")
        w["unequip_animation"] = wd.get("unequip_animation")
        w["pritosec_animation"] = wd.get("pritosec_animation")
        w["sectopri_animation"] = wd.get("sectopri_animation")
        w["aimsettings"] = wd.get("aimsettings")
        w["gunviscmds_symbol"] = wd.get("gunviscmds")
        for k in ("equip_animation", "unequip_animation", "pritosec_animation", "sectopri_animation"):
            closure(w[k])
        for f in w.get("functions") or []:
            if f:
                closure(f.get("fire_animation"))
        for a in w.get("ammo") or []:
            if a:
                closure(a.get("reload_animation"))

    anim_ids: set[int] = set()
    for name in used_scripts:
        for c in scripts[name]["cmds"]:
            if c["op"] == "playanimation" and isinstance(c.get("anim"), int):
                anim_ids.add(c["anim"])

    return {
        "_provenance": "tools/pd-assets/pd_fpgun.py over reference/pd-decomp (ntsc-final)",
        "weapons": table["weapons"],
        "scripts": {k: scripts[k] for k in sorted(used_scripts)},
        "aimsettings": aims,
        "recoilsettings": recoils,
        "noisesettings": noises,
        "gunviscmds": gunvis,
        "anim_ids": sorted(anim_ids),
        "sfx": {k: v for k, v in sfx.items()},
    }, anim_table


# ---------------------------------------------------------------------------
# Animations: raw bins + metadata
# ---------------------------------------------------------------------------

#: The head-bob / walk displacement clips (`g_HeadAnims`, bondhead.c:14) and the
#: stand pose `bhead_reset` measures standheight from (bondheadreset.c:117).
HEAD_ANIMS = ["ANIM_002B", "ANIM_0029", "ANIM_TWO_GUN_HOLD"]
#: Animations the gun code starts from C rather than a weapon script:
#: var80070200 (bondgun.c:6391), the remote mine detonator press.
EXTRA_ANIMS = ["ANIM_0434"]


def export_anims(anim_table: list[dict], ids: list[int], outdir: str) -> dict:
    os.makedirs(outdir, exist_ok=True)
    meta = {}
    for i in ids:
        a = anim_table[i]
        src = os.path.join(ASSETS, "animations", a["file"])
        dst = os.path.join(outdir, f"{i:04x}.bin")
        shutil.copyfile(src, dst)
        flags = (1 if a.get("flag01") else 0) | (2 if a.get("flag02") else 0) | \
                (4 if a.get("flag04") else 0) | (8 if a.get("flag08") else 0)
        meta[str(i)] = {
            "id": a["id"], "file": f"{i:04x}.bin", "numframes": a["numframes"],
            "bytesperframe": a["bytesperframe"], "headerlen": a["unk08"], "framelen": a["unk0a"],
            "flags": flags,
        }
    return meta


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------

#: The player's hands. `g_HeadsAndBodies[bodynum].handfilenum` picks per body;
#: Joanna's default combat outfit is `hand_joaf1`, the MP default Dark body.
HAND_FILES = [
    "hand_joaf1.bin", "hand_carrington.bin", "hand_jotrench.bin", "hand_mrblonde.bin",
    # g_HeadsAndBodies (modeldata/robot.c): Joanna's other outfits. Her combat
    # suit (BODY_DARK_COMBAT) uses FILE_GCOMBATHANDSLOD, exported below.
    "hand_jofrock.bin", "hand_jopilot.bin", "hand_jowetsuit.bin", "hand_josnow.bin",
]


# The third-person models the gun code spawns into the world: every funcdef's
# `projectilemodelnum` resolved through g_ModelStates (modeldata/general.c:404).
PROP_FILES = [
    "chrgrenade.bin",        # MODEL_CHRGRENADE 0x112 (grenade throw/pinball)
    "chrnbomb.bin",          # MODEL_CHRNBOMB 0x110
    "chrtimedmine.bin",      # MODEL_CHRTIMEDMINE 0x113
    "chrproximitymine.bin",  # MODEL_CHRPROXIMITYMINE 0x114
    "chrremotemine.bin",     # MODEL_CHRREMOTEMINE 0x115
    "chrknife.bin",          # MODEL_CHRKNIFE 0x10f (knife throw)
    "chrdyrocketmis.bin",    # MODEL_CHRDYROCKETMIS 0x11f (rocket launcher)
    "chrskrocketmis.bin",    # MODEL_CHRSKROCKETMIS 0x120 (Slayer)
    "chrcrossbolt.bin",      # MODEL_CHRCROSSBOLT 0x121
    "chrdevgrenade.bin",     # MODEL_CHRDEVGRENADE 0x122 (Devastator)
    "chrdraggrenade.bin",    # MODEL_CHRDRAGGRENADE 0x123 (SuperDragon)
    "chrdragon.bin",         # MODEL_CHRDRAGON 0x0ff (Dragon self-destruct)
    "chrautogun.bin",        # MODEL_CHRAUTOGUN 0x157 (Laptop sentry)
]


def export_props(outdir: str) -> int:
    texdir = os.path.join(outdir, "textures")
    modeldir = os.path.join(outdir, "models")
    os.makedirs(texdir, exist_ok=True)
    os.makedirs(modeldir, exist_ok=True)
    for f in PROP_FILES:
        path = os.path.join(ASSETS, "files", "props", f)
        stem = os.path.splitext(f)[0]
        model, warnings = export_model(path, texdir)
        with open(os.path.join(modeldir, stem + ".json"), "w", encoding="utf-8", newline="\n") as fh:
            json.dump(model, fh, separators=(",", ":"))
        nb = sum(len(b["indices"]) // 3 for b in model["batches"])
        print(f"{stem}: {nb} tris, {len(model['materials'])} materials, {len(model['textures'])} textures"
              + (f", {len(warnings)} warnings" if warnings else ""))
        for wmsg in warnings:
            print(f"  WARN {wmsg}", file=sys.stderr)
    return 0


def cmd_all(outdir: str) -> int:
    pd_weapons.require_decomp()
    os.makedirs(outdir, exist_ok=True)
    texdir = os.path.join(outdir, "textures")
    modeldir = os.path.join(outdir, "models")
    os.makedirs(texdir, exist_ok=True)
    os.makedirs(modeldir, exist_ok=True)

    weapons, anim_table = build_weapons()
    anim_index = {a["id"]: i for i, a in enumerate(anim_table)}

    files = []
    for w in weapons["weapons"]:
        fp = (w.get("assets") or {}).get("fp_model")
        if fp and fp not in files:
            files.append(fp)
    files += [f"guns/{h}" for h in HAND_FILES]
    # FILE_GCOMBATHANDSLOD — the unarmed fists are a WEAPON model (bondgun.c:3865).
    files.append("guns/combathandslod.bin")
    # g_CartFileNums (bondgun.c:167): the ejected casings, by ammo casingeject.
    files += ["guns/cartridge.bin", "guns/cartrifle.bin", "guns/cartblue.bin", "guns/cartshell.bin"]

    models_meta = {}
    all_warn = []
    for rel in files:
        path = os.path.join(ASSETS, "files", rel)
        if not os.path.exists(path):
            print(f"  missing {rel}", file=sys.stderr)
            continue
        stem = os.path.splitext(os.path.basename(rel))[0]
        model, warnings = export_model(path, texdir)
        with open(os.path.join(modeldir, stem + ".json"), "w", encoding="utf-8", newline="\n") as fh:
            json.dump(model, fh, separators=(",", ":"))
        nb = sum(len(b["indices"]) // 3 for b in model["batches"])
        models_meta[stem] = {"file": f"models/{stem}.json", "tris": nb, "materials": len(model["materials"]),
                             "textures": len(model["textures"])}
        for wmsg in warnings:
            all_warn.append(f"{stem}: {wmsg}")
        print(f"{stem}: {nb} tris, {len(model['materials'])} materials, {len(model['textures'])} textures"
              + (f", {len(warnings)} warnings" if warnings else ""))

    ids = sorted(set(weapons["anim_ids"]) | {anim_index[a] for a in HEAD_ANIMS + EXTRA_ANIMS})
    anims_meta = export_anims(anim_table, ids, os.path.join(outdir, "anims"))
    weapons["anims"] = anims_meta
    weapons["models"] = models_meta
    weapons["head_anims"] = {a: anim_index[a] for a in HEAD_ANIMS}
    with open(os.path.join(outdir, "weapons.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(weapons, fh, indent=1)
    print(f"weapons.json: {len(weapons['weapons'])} weapons, {len(weapons['scripts'])} scripts, "
          f"{len(anims_meta)} anims")
    if all_warn:
        print("warnings:\n  " + "\n  ".join(all_warn[:40]), file=sys.stderr)
    return export_props(outdir)


def cmd_extra_anims(outdir: str) -> int:
    """Add EXTRA_ANIMS to an existing export without redoing the rest."""
    anim_table, anim_index = load_anim_table()
    ids = [anim_index[a] for a in EXTRA_ANIMS]
    meta = export_anims(anim_table, ids, os.path.join(outdir, "anims"))
    path = os.path.join(outdir, "weapons.json")
    with open(path, encoding="utf-8") as fh:
        weapons = json.load(fh)
    weapons["anims"].update(meta)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(weapons, fh, indent=1)
    print(f"added {', '.join(EXTRA_ANIMS)} -> {sorted(meta)}")
    return 0


def cmd_dl(path: str) -> int:
    tmp = os.path.join(os.environ.get("TEMP", "."), "pd_fpgun_tex")
    os.makedirs(tmp, exist_ok=True)
    model, warnings = export_model(path, tmp)
    for i, mat in enumerate(model["materials"]):
        print(f"mat {i}: {json.dumps(mat)}")
    for b in model["batches"]:
        node = model["nodes"][b["node"]]
        print(f"batch node {b['node']} ({node.get('partnum', '')}) mat {b['material']}: "
              f"{len(b['indices']) // 3} tris, mtx {sorted(set(v[3] for v in b['verts']))}")
    for wmsg in warnings:
        print("WARN", wmsg)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("all")
    p.add_argument("outdir", nargs="?", default=DEFAULT_OUT)
    p = sub.add_parser("model")
    p.add_argument("path")
    p.add_argument("out")
    p = sub.add_parser("dl")
    p.add_argument("path")
    p = sub.add_parser("props", help="export only the world projectile models (PROP_FILES)")
    p.add_argument("outdir", nargs="?", default=DEFAULT_OUT)
    p = sub.add_parser("extra-anims", help="add EXTRA_ANIMS to an existing export")
    p.add_argument("outdir", nargs="?", default=DEFAULT_OUT)
    args = ap.parse_args()
    if args.cmd == "extra-anims":
        return cmd_extra_anims(args.outdir)
    if args.cmd == "all":
        return cmd_all(args.outdir)
    if args.cmd == "props":
        return export_props(args.outdir)
    if args.cmd == "model":
        texdir = os.path.dirname(os.path.abspath(args.out))
        model, warnings = export_model(args.path, texdir)
        with open(args.out, "w", encoding="utf-8", newline="\n") as fh:
            json.dump(model, fh, indent=1)
        for wmsg in warnings:
            print("WARN", wmsg, file=sys.stderr)
        return 0
    if args.cmd == "dl":
        return cmd_dl(args.path)
    return 1


if __name__ == "__main__":
    sys.exit(main())
