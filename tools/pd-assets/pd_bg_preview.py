#!/usr/bin/env python3
"""Render a `pd_bg.py` export (bg.json + textures) to a PNG, on the CPU.

An independent reader of the exported data: it re-implements, from the JSON
alone, the parts of the RDP a BG batch uses — the colour combiner (fast3d mux
order, both cycles, `(a - b) * c + d`), the texture tile (texel UVs, the `uls`
half-texel offset, wrap / clamp / mirror), back/front culling, the z-buffer and
alpha blending — so a wrong material, UV or winding shows up on screen instead
of hiding behind a structural check. It shares nothing with the exporter but
the PNG writer. Level-0 textures only, nearest sampling, LOD fraction 0.

Usage:
    python pd_bg_preview.py <bg.json> <out.png> --eye X Y Z --at X Y Z [--fov 60] [--size 640 480]
    python pd_bg_preview.py <bg.json> <out.png> --pad PAD_REF_001C [--pads pads/ref.json] [--eye-height 159]
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from pd_gltf import png_bytes  # noqa: E402

try:
    from PIL import Image
except ImportError:  # pragma: no cover
    Image = None

REPO = os.path.dirname(os.path.dirname(HERE))
ASSETS = os.path.join(REPO, "reference", "pd-decomp", "src", "assets", "ntsc-final")


def load_png(path: str) -> np.ndarray:
    im = Image.open(path).convert("RGBA")
    return np.asarray(im, dtype=np.float32) / 255.0


def look_at(eye, at, up=(0.0, 1.0, 0.0)):
    eye, at, up = np.array(eye, float), np.array(at, float), np.array(up, float)
    f = at - eye
    f /= np.linalg.norm(f)
    if abs(np.dot(f, up)) > 0.999:
        up = np.array([0.0, 0.0, -1.0])
    r = np.cross(f, up)
    r /= np.linalg.norm(r)
    u = np.cross(r, f)
    m = np.eye(4)
    m[0, :3], m[1, :3], m[2, :3] = r, u, -f
    m[:3, 3] = -m[:3, :3] @ eye
    return m


class Tex:
    def __init__(self, img: np.ndarray):
        self.img = img
        self.h, self.w = img.shape[:2]

    @staticmethod
    def _wrap(i, n, mode):
        if mode == 1:  # clamp
            return np.clip(i, 0, n - 1)
        if mode == 2:  # mirror
            p = np.mod(i, 2 * n)
            return np.where(p >= n, 2 * n - 1 - p, p)
        return np.mod(i, n)

    def sample(self, s, t, tex):
        # Texel space: fast3d subtracts the tile's uls/ult; shifts are 0 in BG.
        ss = (s - tex["uls"]) / (2 ** tex["shifts"] if tex["shifts"] <= 10 else 1)
        tt = (t - tex["ult"]) / (2 ** tex["shiftt"] if tex["shiftt"] <= 10 else 1)
        xi = self._wrap(np.floor(ss).astype(np.int64), self.w, tex["cms"])
        yi = self._wrap(np.floor(tt).astype(np.int64), self.h, tex["cmt"])
        return self.img[yi, xi]


def combine(mat, shade, t0, env, prim):
    """The RDP colour combiner, both cycles (fast3d `color_comb` mux order)."""
    n = shade.shape[0]
    one = np.ones((n, 3), np.float32)
    zero = np.zeros((n, 3), np.float32)
    envc = np.broadcast_to(np.array(env[:3], np.float32) / 255.0, (n, 3))
    enva = np.full(n, env[3] / 255.0, np.float32)
    primc = np.broadcast_to(np.array(prim[:3], np.float32) / 255.0, (n, 3))
    prima = np.full(n, prim[3] / 255.0, np.float32)
    comb_c, comb_a = zero, np.zeros(n, np.float32)
    mux = mat["combine"]
    cycles = (0, 1) if mat["two_cycle"] else (0,)
    for cyc in cycles:
        a, b, c, d, Aa, Ab, Ac, Ad = mux[8 * cyc : 8 * cyc + 8]
        base = {0: comb_c, 1: t0[:, :3], 2: t0[:, :3], 3: primc, 4: shade[:, :3], 5: envc}

        def col(i, kind):
            if i in base:
                return base[i]
            if kind == "a":
                return one if i == 6 else zero
            if kind == "b":
                return zero
            if kind == "d":
                return one if i == 6 else zero
            # c
            m = {6: zero, 7: comb_a, 8: t0[:, 3], 9: t0[:, 3], 10: prima, 11: shade[:, 3], 12: enva,
                 13: np.zeros(n, np.float32), 14: np.zeros(n, np.float32), 15: np.zeros(n, np.float32)}
            v = m.get(i, np.zeros(n, np.float32))
            return v[:, None] if v.ndim == 1 else v

        abase = {0: comb_a, 1: t0[:, 3], 2: t0[:, 3], 3: prima, 4: shade[:, 3], 5: enva,
                 6: np.ones(n, np.float32), 7: np.zeros(n, np.float32)}

        def alp(i, is_c):
            if is_c:
                if i == 0:
                    return np.zeros(n, np.float32)  # LOD_FRACTION
                if i == 6:
                    return np.zeros(n, np.float32)  # PRIM_LOD_FRAC
            return abase[i]

        rc = np.clip((col(a, "a") - col(b, "b")) * col(c, "c") + col(d, "d"), 0, 1)
        ra = np.clip((alp(Aa, False) - alp(Ab, False)) * alp(Ac, True) + alp(Ad, False), 0, 1)
        comb_c, comb_a = rc, ra
    return comb_c, comb_a


def render(model: dict, texdir: str, eye, at, fov=60.0, size=(640, 480), near=5.0, nocull=False,
           rooms=None):
    W, H = size
    view = look_at(eye, at)
    f = 1.0 / math.tan(math.radians(fov) / 2)
    aspect = W / H
    color = np.zeros((H, W, 3), np.float32)
    color[:] = (0.02, 0.0, 0.0)  # Complex's sky colour 0x020000 (env.c:105)
    depth = np.full((H, W), np.inf, np.float32)
    texcache: dict[int, Tex] = {}
    mats = model["materials"]
    order = {i: k for k, i in enumerate(range(len(model["nodes"])))}
    batches = sorted(model["batches"], key=lambda b: order[b["node"]])
    ntri = culled = 0
    for b in batches:
        if rooms is not None and model["nodes"][b["node"]]["room"] not in rooms:
            continue
        mat = mats[b["material"]]
        tex = mat["texture"]
        tobj = None
        if tex is not None:
            tid = tex["id"]
            if tid not in texcache:
                ref = model["textures"].get(str(tid))
                texcache[tid] = Tex(load_png(os.path.join(texdir, ref["file"]))) if ref else None
            tobj = texcache[tid]
        V = np.array(b["verts"], np.float64)
        P = np.c_[V[:, :3], np.ones(len(V))] @ view.T  # camera space (looking down -z)
        idx = np.array(b["indices"], np.int64).reshape(-1, 3)
        env = mat["env"] or [255, 255, 255, 255]
        prim = mat["prim"]
        for tri in idx:
            ntri += 1
            poly = [(P[i, :3], V[i, 4:6], V[i, 6:10] / 255.0) for i in tri]
            # Clip against the near plane z = -near (Sutherland-Hodgman).
            out = []
            for k in range(len(poly)):
                cur, nxt = poly[k], poly[(k + 1) % len(poly)]
                cin, nin = cur[0][2] <= -near, nxt[0][2] <= -near
                if cin:
                    out.append(cur)
                if cin != nin:
                    tt = (-near - cur[0][2]) / (nxt[0][2] - cur[0][2])
                    out.append(tuple(cur[j] + (nxt[j] - cur[j]) * tt for j in range(3)))
            if len(out) < 3:
                continue
            pts = []
            for p, uv, c in out:
                w = -p[2]
                sx = (p[0] * f / aspect / w * 0.5 + 0.5) * W
                sy = (0.5 - p[1] * f / w * 0.5) * H
                pts.append((sx, sy, 1.0 / w, uv, c))
            # Winding in screen space (y down): N64/OpenGL front = CCW in y-up.
            (x0, y0), (x1, y1), (x2, y2) = [(q[0], q[1]) for q in pts[:3]]
            area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)
            front = area < 0  # CCW with y up == negative area with y down
            cull = mat["cull"]
            if nocull:
                cull = "none"
            if (cull == "back" and not front) or (cull == "front" and front) or cull == "both":
                culled += 1
                continue
            for k in range(1, len(pts) - 1):
                raster(pts[0], pts[k], pts[k + 1], mat, tobj, tex, env, prim, color, depth)
    return color, ntri, culled


def raster(p0, p1, p2, mat, tobj, tex, env, prim, color, depth):
    H, W = depth.shape
    xs = [p0[0], p1[0], p2[0]]
    ys = [p0[1], p1[1], p2[1]]
    xmin, xmax = max(int(math.floor(min(xs))), 0), min(int(math.ceil(max(xs))), W - 1)
    ymin, ymax = max(int(math.floor(min(ys))), 0), min(int(math.ceil(max(ys))), H - 1)
    if xmin > xmax or ymin > ymax:
        return
    area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p2[0] - p0[0]) * (p1[1] - p0[1])
    if abs(area) < 1e-9:
        return
    gx, gy = np.meshgrid(np.arange(xmin, xmax + 1) + 0.5, np.arange(ymin, ymax + 1) + 0.5)
    w0 = ((p1[0] - gx) * (p2[1] - gy) - (p2[0] - gx) * (p1[1] - gy)) / area
    w1 = ((p2[0] - gx) * (p0[1] - gy) - (p0[0] - gx) * (p2[1] - gy)) / area
    w2 = 1.0 - w0 - w1
    inside = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
    if not inside.any():
        return
    yy, xx = np.nonzero(inside)
    b0, b1, b2 = w0[inside], w1[inside], w2[inside]
    iw = b0 * p0[2] + b1 * p1[2] + b2 * p2[2]
    z = 1.0 / iw
    py, px = yy + ymin, xx + xmin
    ztest = z <= depth[py, px] + 0.01 if mat["ztest"] else np.ones_like(z, bool)
    if not ztest.any():
        return
    py, px, b0, b1, b2, z, iw = py[ztest], px[ztest], b0[ztest], b1[ztest], b2[ztest], z[ztest], iw[ztest]

    def interp(a0, a1, a2):
        a0, a1, a2 = np.asarray(a0), np.asarray(a1), np.asarray(a2)
        return ((b0[:, None] * a0 * p0[2] + b1[:, None] * a1 * p1[2] + b2[:, None] * a2 * p2[2])
                / iw[:, None])

    uv = interp(p0[3], p1[3], p2[3])
    shade = interp(p0[4], p1[4], p2[4]).astype(np.float32)
    if tobj is not None:
        t0 = tobj.sample(uv[:, 0], uv[:, 1], tex).astype(np.float32)
    else:
        t0 = np.ones((len(z), 4), np.float32)
    rgb, a = combine(mat, shade, t0, env, prim)
    if mat["blend"] == "alpha":
        dst = color[py, px]
        color[py, px] = rgb * a[:, None] + dst * (1 - a[:, None])
    else:
        color[py, px] = rgb
    if mat["zwrite"]:
        depth[py, px] = np.minimum(depth[py, px], z)


def pad_view(pad_id: str, pads_path: str, tiles_path: str, eye_height: float):
    with open(pads_path, encoding="utf-8") as fh:
        pads = {p["id"]: p for p in json.load(fh)["pads"]}
    p = pads[pad_id]
    x, y, z = p["pos"]
    floor = find_floor(tiles_path, x, y + 50, z)
    fy = floor if floor is not None else y
    eye = (x, fy + eye_height, z)
    dx, dy, dz = p["dir"]
    at = (x + dx * 100, fy + eye_height, z + dz * 100)
    return eye, at, fy


def find_floor(tiles_path: str, x: float, ymax: float, z: float):
    with open(tiles_path, encoding="utf-8") as fh:
        rooms = json.load(fh)["rooms"]
    best = None
    for tiles in rooms.values():
        for t in tiles:
            vs = [(v["x"], v["y"], v["z"]) for v in t["vertices"]]
            if len(vs) < 3:
                continue
            # point in polygon (xz)
            inside = False
            n = len(vs)
            for i in range(n):
                xi, _, zi = vs[i]
                xj, _, zj = vs[(i + 1) % n]
                if (zi > z) != (zj > z) and x < (xj - xi) * (z - zi) / (zj - zi + 1e-12) + xi:
                    inside = not inside
            if not inside:
                continue
            a, b, c = np.array(vs[0], float), np.array(vs[1], float), np.array(vs[2], float)
            nrm = np.cross(b - a, c - a)
            if abs(nrm[1]) < 1e-6:
                continue
            fy = a[1] - (nrm[0] * (x - a[0]) + nrm[2] * (z - a[2])) / nrm[1]
            if fy <= ymax and (best is None or fy > best):
                best = fy
    return best


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("bg")
    ap.add_argument("out")
    ap.add_argument("--eye", nargs=3, type=float)
    ap.add_argument("--at", nargs=3, type=float)
    ap.add_argument("--pad")
    ap.add_argument("--pads", default=os.path.join(ASSETS, "pads", "ref.json"))
    ap.add_argument("--tiles", default=os.path.join(ASSETS, "tiles", "ref.json"))
    ap.add_argument("--eye-height", type=float, default=159.0)
    ap.add_argument("--fov", type=float, default=60.0)
    ap.add_argument("--size", nargs=2, type=int, default=[640, 480])
    ap.add_argument("--nocull", action="store_true", help="debug: draw back faces too")
    ap.add_argument("--rooms", type=int, nargs="*", help="debug: only these rooms")
    args = ap.parse_args()
    with open(args.bg, encoding="utf-8") as fh:
        model = json.load(fh)
    texdir = os.path.join(os.path.dirname(os.path.abspath(args.bg)), "textures")
    if args.pad:
        eye, at, fy = pad_view(args.pad, args.pads, args.tiles, args.eye_height)
        print(f"{args.pad}: floor y {fy:.1f}, eye {tuple(round(v, 1) for v in eye)}")
    else:
        eye, at = args.eye, args.at
    img, ntri, culled = render(model, texdir, eye, at, args.fov, tuple(args.size), nocull=args.nocull,
                               rooms=set(args.rooms) if args.rooms else None)
    rgba = np.concatenate([img, np.ones(img.shape[:2] + (1,), np.float32)], axis=2)
    data = (np.clip(rgba, 0, 1) * 255 + 0.5).astype(np.uint8).tobytes()
    with open(args.out, "wb") as fh:
        fh.write(png_bytes(img.shape[1], img.shape[0], data))
    print(f"{args.out}: {ntri} tris considered, {culled} culled")
    return 0


if __name__ == "__main__":
    sys.exit(main())
