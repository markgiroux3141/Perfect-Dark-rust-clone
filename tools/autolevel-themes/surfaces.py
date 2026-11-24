"""Where a stage draws each of its materials: area on floors, slopes, walls and ceilings, and
for walls the band they sit in (height above the floor of their room under them). For a theme:
run it on the arena (what its textures are for) and on a themed level (what each role is for).

Usage: python tools/autolevel-themes/surfaces.py STAGE [--custom] [--bands]
  STAGE: assets/stages/STAGE (an arena), or with --custom custom/stages/STAGE (an import).
  --bands: each material's wall area by height above the floor, in 59 cm steps."""
import json, math, os, struct, sys
from collections import defaultdict

repo = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
args = [a for a in sys.argv[1:] if not a.startswith("--")]
if len(args) != 1:
    sys.exit(__doc__)
st = args[0]
base = os.path.join(repo, "custom" if "--custom" in sys.argv else "assets", "stages", st)
bg = json.load(open(os.path.join(base, "bg.json")))
data = open(os.path.join(base, "bg.bin"), "rb").read()

tris = []  # (material, room, verts, area, ny)
off = 0
for b in bg["batches"]:
    room = bg["nodes"][b["node"]].get("room")
    vs = [struct.unpack_from("<fff", data, off + 28 * i) for i in range(b["nverts"])]
    off += 28 * b["nverts"]
    idx = struct.unpack_from(f"<{b['nidx']}H", data, off)
    off += 2 * b["nidx"]
    for t in range(0, len(idx), 3):
        a, c, d = (vs[k] for k in idx[t:t + 3])
        e1 = [c[k] - a[k] for k in range(3)]
        e2 = [d[k] - a[k] for k in range(3)]
        n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]
        l = math.sqrt(sum(q * q for q in n))
        if l > 0:
            tris.append((b["material"], room, (a, c, d), l / 2, n[1] / l))

def kind(ny):
    return "floor" if ny > 0.95 else "slope" if ny > 0.3 else "ceil" if ny < -0.95 else "under" if ny < -0.3 else "wall"

# A room's floor levels: the heights of its flat floors (to the cm), by area.
floors = defaultdict(set)
for m, room, v, area, ny in tris:
    if ny > 0.95 and area > 2000:
        floors[room].add(round(sum(p[1] for p in v) / 3))

def above(room, y):
    under = [f for f in floors.get(room, ()) if f <= y + 2]
    return y - max(under) if under else None

total = sum(t[3] for t in tris)
by = defaultdict(lambda: defaultdict(float))
bands = defaultdict(lambda: defaultdict(float))
for m, room, v, area, ny in tris:
    k = kind(ny)
    by[m][k] += area
    if k == "wall":
        h = above(room, min(p[1] for p in v))
        bands[m]["?" if h is None else int(h // 59) * 59] += area

def tex(m):
    t = bg["materials"][m]["texture"]
    return f"{t['id']:04X}" if t else "----"

print(f"{st}: {len(bg['materials'])} materials, % of all area by facing (floor, slope, wall, under, ceil)")
for m in sorted(by, key=lambda m: -sum(by[m].values())):
    row = bg["materials"][m]
    flags = ("2s " if row["cull"] == "none" else "") + ("clr" if row["zwrite"] is False else "")
    cells = "  ".join(f"{k} {100 * by[m][k] / total:5.1f}" for k in ("floor", "slope", "wall", "under", "ceil") if by[m][k] > total * 0.0005)
    print(f"{m:3} {tex(m)} {flags:6} {cells}")
    if "--bands" in sys.argv and bands[m]:
        w = sum(bands[m].values())
        top = sorted(bands[m].items(), key=lambda kv: -kv[1])[:6]
        print("            walls from:", ", ".join(f"{h}cm {100 * a / w:.0f}%" for h, a in sorted(top, key=lambda kv: (str(kv[0]) if kv[0] == '?' else f'{kv[0]:06d}'))))
