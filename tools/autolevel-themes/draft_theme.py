"""Draft a texture theme for the generated levels from an arena's own use of its textures:
crates/pd_import/themes/NAME.json (pd_import::themes), to be tuned by hand after. A theme
already there is not written over unless --force.

Usage: python tools/autolevel-themes/draft_theme.py STAGE NAME TITLE [--force]
  e.g. draft_theme.py jun temple Temple; then: autolevel import DIR --theme temple"""
import json, math, os, struct, sys

repo = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
root = f"{repo}/assets"
args = [a for a in sys.argv[1:] if a != "--force"]
if len(args) != 3:
    sys.exit(__doc__)
st, name, title = args
out = f"{repo}/crates/pd_import/themes/{name}.json"
if os.path.exists(out) and "--force" not in sys.argv:
    sys.exit(f"{out} is there (tuned by hand?): --force to draft it again")
bg = json.load(open(f"{root}/stages/{st}/bg.json"))
data = open(f"{root}/stages/{st}/bg.bin", "rb").read()
off = 0
area = {}
for b in bg["batches"]:
    vs = [struct.unpack_from("<fff", data, off + 28 * i) for i in range(b["nverts"])]
    alphas = [data[off + 28 * i + 25] for i in range(b["nverts"])]
    off += 28 * b["nverts"]
    idx = struct.unpack_from(f"<{b['nidx']}H", data, off)
    off += 2 * b["nidx"]
    m = bg["materials"][b["material"]]
    if not m["texture"]:
        continue
    tid = m["texture"]["id"]
    clear = m["zwrite"] is False
    s = area.setdefault((tid, clear), {"floor": 0.0, "wall": 0.0, "ceil": 0.0, "wraps": False, "faded": True})
    # A row whose alpha is shade alpha (cycle 2's alpha C is 4) is as see-through as its
    # vertices: Temple's light shafts are a white texture faded by vertex alpha (0..77). The
    # bake's vertices are opaque (255), so such a texture only works as glass if its own
    # arena draws it somewhere at 255 (Skedar's 0291); else it would draw solid white.
    if m["combine"][14] != 4 or 255 in alphas:
        s["faded"] = False
    if m["texture"]["cms"] != 1 and m["texture"]["cmt"] != 1:
        s["wraps"] = True
    for t in range(0, len(idx), 3):
        a, c, d = (vs[k] for k in idx[t:t + 3])
        e1 = [c[k] - a[k] for k in range(3)]
        e2 = [d[k] - a[k] for k in range(3)]
        n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]
        l = math.sqrt(sum(q * q for q in n))
        if l == 0:
            continue
        ny = n[1] / l
        s["floor" if ny > 0.7 else "ceil" if ny < -0.7 else "wall"] += l / 2

def size(tid):
    t = bg["textures"].get(str(tid), {})
    return t.get("w", 0), t.get("h", 0)

def ranked(kind, clear=False, wide=False, wraps=False):
    out = [(s[kind], tid) for (tid, c), s in area.items() if c == clear and s[kind] > 0 and size(tid)[0] >= 8 and (s["wraps"] or not wraps) and not (clear and s["faded"])]
    if wide:
        out = [o for o in out if size(o[1])[0] >= 32] or out
    return [tid for _, tid in sorted(out, reverse=True)]

h = lambda t: f"{t:04X}"
walls = ranked("wall", wide=True)
# (a floor, a deck or a ceiling is a wide plane: a texture clamped either way smears across it)
floors = ranked("floor", wraps=True) or ranked("floor")
ceils = ranked("ceil", wraps=True) or ranked("wall", wraps=True) or ranked("ceil") or walls
roles = {}
if floors:
    roles["floor"] = h(floors[0])
    roles["grate"] = h(floors[0])  # (Complex's decks are its floor's texture, both-sided)
strips = ["plain", "panel", "panel_stripe", "pipes", "base_pipe", "panel_pipes", "grille", "door_panel", "grey_panel", "grey_pipe"]
for k, role in enumerate(strips):
    if walls:
        roles[role] = h(walls[k % len(walls)])
roles["ceiling_tile"] = h(ceils[0])
roles["ceiling_dark"] = h(ceils[1] if len(ceils) > 1 else ceils[0])
roles["vent"] = h(ceils[1] if len(ceils) > 1 else ceils[0])
glass = ranked("wall", clear=True)
if glass:
    roles["window"] = roles["rail"] = h(glass[0])
mesh = ranked("floor", clear=True)
if mesh:
    roles["mesh"] = h(mesh[0])

theme = {"format": "pd-import-theme/1", "name": title, "stage": st, "roles": roles}
os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, "w", newline="") as f:
    f.write(json.dumps(theme, indent=2) + "\n")
print(out, json.dumps(roles))
