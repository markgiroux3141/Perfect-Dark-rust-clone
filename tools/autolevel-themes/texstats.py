"""Per material of a stage: area on floors / walls / ceilings, units per texture repeat, and a
contact sheet of its textures. Usage: python tools/autolevel-themes/texstats.py STAGE OUT.png"""
import json, math, os, struct, sys
from PIL import Image, ImageDraw

root = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "assets")
st, out = sys.argv[1], sys.argv[2]
bg = json.load(open(f"{root}/stages/{st}/bg.json"))
data = open(f"{root}/stages/{st}/bg.bin", "rb").read()
off = 0
stats = {}
for b in bg["batches"]:
    vs = []
    for i in range(b["nverts"]):
        x, y, z, mtx, u, v = struct.unpack_from("<fffHff", data, off + 28 * i)
        vs.append((x, y, z, u, v))
    off += 28 * b["nverts"]
    idx = struct.unpack_from(f"<{b['nidx']}H", data, off)
    off += 2 * b["nidx"]
    s = stats.setdefault(b["material"], {"floor": 0.0, "wall": 0.0, "ceil": 0.0, "upr": []})
    for t in range(0, len(idx), 3):
        a, c, d = (vs[k] for k in idx[t:t + 3])
        e1 = [c[k] - a[k] for k in range(3)]
        e2 = [d[k] - a[k] for k in range(3)]
        n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]]
        area2 = math.sqrt(sum(q * q for q in n))
        if area2 == 0:
            continue
        ny = n[1] / area2
        area = area2 / 2
        kind = "floor" if ny > 0.7 else "ceil" if ny < -0.7 else "wall"
        s[kind] += area
        # units per texture repeat along e1 (uv in texels*? PD uv are in texels)
        du = math.hypot(c[3] - a[3], c[4] - a[4])
        L = math.sqrt(sum(q * q for q in e1))
        if du > 0:
            s["upr"].append(L / du)
    off += 0

mats = bg["materials"]
rows = []
for i, m in enumerate(mats):
    s = stats.get(i, {"floor": 0, "wall": 0, "ceil": 0, "upr": []})
    tid = m["texture"]["id"] if m["texture"] else 0
    tr = bg["textures"].get(str(tid), {})
    upr = sorted(s["upr"])
    med = upr[len(upr) // 2] if upr else 0
    # units per texel -> units per repeat along width
    rows.append((i, tid, m["cull"], m["zwrite"], tr.get("w"), tr.get("h"), s["floor"] / 1e4, s["wall"] / 1e4, s["ceil"] / 1e4, med * (tr.get("w") or 1)))
tot = sum(r[6] + r[7] + r[8] for r in rows)
print(f"{st}: {len(mats)} materials, area {tot:.0f} (x1e4 units^2)")
print(" i   tex   cull  zw   w  h   floor%  wall%  ceil%  units/repeat(w)")
for r in rows:
    print(f"{r[0]:2} {r[1]:#06x} {r[2]:5} {str(r[3])[0]} {r[4]:3} {r[5]:3}  {100*r[6]/tot:6.1f} {100*r[7]/tot:6.1f} {100*r[8]/tot:6.1f}   {r[9]:7.1f}")

# contact sheet
S = 128
cols = 6
n = len(rows)
sheet = Image.new("RGB", (cols * (S + 8), ((n + cols - 1) // cols) * (S + 30)), (40, 40, 40))
dr = ImageDraw.Draw(sheet)
for k, r in enumerate(rows):
    tr = bg["textures"].get(str(r[1]))
    if not tr:
        continue
    im = Image.open(f"{root}/{tr['file']}").convert("RGBA")
    # tile it 2x2 to show the repeat
    w, h = im.size
    t = Image.new("RGBA", (2 * w, 2 * h))
    for a in range(2):
        for b in range(2):
            t.paste(im, (a * w, b * h))
    sc = S / max(2 * w, 2 * h)
    t = t.resize((max(1, int(2 * w * sc)), max(1, int(2 * h * sc))), Image.NEAREST)
    x, y = (k % cols) * (S + 8) + 4, (k // cols) * (S + 30) + 4
    bgc = Image.new("RGBA", t.size, (255, 0, 255, 255))
    sheet.paste(Image.alpha_composite(bgc, t).convert("RGB"), (x, y))
    kind = max(("F", r[6]), ("W", r[7]), ("C", r[8]), key=lambda q: q[1])[0]
    dr.text((x, y + S + 2), f"{r[0]} {r[1]:04X} {kind} {'' if r[3] else 'clr'}{' 2s' if r[2]=='none' else ''}", fill=(255, 255, 255))
    dr.text((x, y + S + 14), f"F{100*r[6]/tot:.0f} W{100*r[7]/tot:.0f} C{100*r[8]/tot:.0f}", fill=(200, 200, 200))
sheet.save(out)
print("wrote", out)
