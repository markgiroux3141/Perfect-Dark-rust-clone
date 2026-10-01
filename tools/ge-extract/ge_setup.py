"""A GoldenEye setup's doors (and the places it marks) in PD's setup terms.

The Combat Simulator takes a converted level's architecture and nothing of its
missions, so of a setup's props only the doors are kept: PD's doors are GE's
(`struct doorobj` descends from GE's `DoorRecord`, and PD's
`setup_create_door` from GE's `setupDoor`, `prop.c:925`: the model's box
stretched to the pad's, the same axes), so each becomes a `door` row of our
`setup.json` field for field. The rest (guards, weapons, ammo, armour, the
player's starts) only marks spots that placement prefers (`pd_import::place`).

The setup is read as the decomp holds it (`assets/obseg/setup/<name>.c`,
generated from the ROM by Getools), as `pd_stage.py` reads PD's: the pad
lists as C initialisers, `propDefs[]` and `intro[]` as words, each record
opened by the generator's `/* Type = <type>; index = <n> */` comment.

Door record fields (`bondtypes.h:2778`; `setupDoor` converts them,
`prop.c:1113-1124`): word 1 `obj << 16 | pad` (the pad a bound pad,
`prop.c:967`), 2 flags, 3 flags2, 29 `maxdamage << 16`, 32 the linked door
(relative), 33-37 maxfrac, perimfrac, accel, decel, maxspeed (16.16), 38
`doorflags << 16 | doortype`, 39 keyflags, 40 autoclose frames, 41 the open
sound, 48 and 49 the glass's fade distances (`TintDist`, and the s32 at 0xc4
that `glassCalculateOpacity` takes for the opaque one, `propobj.c:5830`).
"""

from __future__ import annotations

import math
import re
import struct

from ge_rom import Ge, GeError

#: GE `PROPFLAG_*` (bondconstants.h) that PD numbers differently.
GE_DOOR_TWOWAY = 0x08000000  # PD OBJFLAG_DOOR_TWOWAY 0x80000000
GE_NO_PORTAL_CLOSE = 0x40000000  # PD's bit here is OBJFLAG_DOOR_KEEPOPEN
GE_DOOR_KEEPOPEN = 0x80000000  # PD OBJFLAG_DOOR_KEEPOPEN 0x40000000
PD_DOOR_TWOWAY, PD_DOOR_KEEPOPEN = 0x80000000, 0x40000000
#: flags2: GE's "position at pad" (0x1, `prop.c:1072`) and "not in
#: multiplayer" (0x8) are PD's OBJFLAG2_IMMUNETOANTI / DOOR_PENDINGACTIVATION.
GE_FLAGS2_DROP = 0x1 | 0x8
#: flags2 LOCKEDFRONT / LOCKEDBACK and AIRLOCKDOOR: the same bits in both games.
FLAGS2_LOCKS = 0x08000000 | 0x10000000
FLAGS2_AIRLOCK = 0x40000000
PADFLAG_HASBBOXDATA = 0x0200  # PD constants.h:3330

#: GE's door sound type (`DOOR_OPEN_SOUND_*`, `doorPlayOpenSound0` ..
#: `doorPlayCloseSound1`, `propobj.c:12734-13160`) as PD's `soundtype`
#: (`door_play_opening_sound` .., PD `propobj.c:18485`). PD's table is GE's
#: with a row inserted at 5 (GE 1-4 are PD 1-4, GE 5-17 are PD 6-18), and PD
#: kept GE's sound numbers; where a PD row's samples are no longer GE's the
#: row with GE's samples is taken, found by comparing the two banks' samples
#: (GE's `sfx.ctl/tbl` from the ROM against PD's): GE 4's heavy slide (214,
#: 215) is PD 17's (PD 4 adds PD's loop 216, a different sample: Runway's
#: rolling door), and GE 3's metal slide is GE 214/215 lower, PD 17 again.
#: GE's own loops 211 and 216 are not in PD's bank.
GE_DOOR_SOUNDTYPE = {0: 0, 1: 1, 2: 2, 3: 17, 4: 17, 5: 6, 6: 7, 7: 8, 8: 9, 9: 10, 10: 11, 11: 12, 12: 13,
                     13: 14, 14: 15, 15: 16, 16: 17, 17: 18, 18: 0}


def split_top(s: str) -> list[str]:
    out, depth, cur = [], 0, []
    for ch in s:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            out.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    if "".join(cur).strip():
        out.append("".join(cur).strip())
    return out


def word(t: str) -> int:
    """One initialiser as the u32 it compiles to (`_mkword`, `_mkshort`: 16- and
    8-bit halves, `bondaicommands.h`; floats as their bits)."""
    t = t.strip()
    m = re.fullmatch(r"_mk(word|short)\((.*)\)", t, re.S)
    if m:
        a, b = (word(x) for x in split_top(m.group(2)))
        return ((a << 16) | (b & 0xFFFF)) & 0xFFFFFFFF if m.group(1) == "word" else ((a << 8) | (b & 0xFF)) & 0xFFFF
    if re.fullmatch(r"-?[0-9.]+(e-?\d+)?f?", t) and ("." in t or "e" in t or t.endswith("f")) and not t.startswith("0x"):
        return struct.unpack(">I", struct.pack(">f", float(t.rstrip("f"))))[0]
    # A pointer (`&credits_data_0`, a solo setup's intro): nothing kept reads it.
    if re.fullmatch(r"&?[A-Za-z_]\w*", t):
        return 0
    return int(t, 0) & 0xFFFFFFFF


def s32(w: int) -> int:
    return w - (1 << 32) if w & 0x80000000 else w


def block(src: str, decl: str) -> str:
    i = src.index(decl)
    return src[src.index("{", i) + 1 : src.index("\n};", i)]


def records(src: str, decl: str) -> list[tuple[str, list[int]]]:
    """`(type, words)` for each record of the word list `decl`."""
    body = block(src, decl)
    parts = re.split(r"/\*\s*Type\s*=\s*(\w+);\s*index\s*=\s*\d+\s*\*/", body)
    out = []
    for i in range(1, len(parts), 2):
        text = re.sub(r"/\*.*?\*/", "", parts[i + 1], flags=re.S)
        out.append((parts[i], [word(t) for t in split_top(text.replace("\n", " "))]))
    return out


def pads(src: str, decl: str, bound: bool) -> list[dict]:
    """`{pos, up, look[, bbox]}` per pad (`PadRecord` / `BoundPadRecord`,
    `bondtypes.h:1663`: pos, up, look, the stan tile's name, its pointer, then
    a bound pad's `{xmin, xmax, ymin, ymax, zmin, zmax}`)."""
    out = []
    for line in block(src, decl).splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        f = [float(x.rstrip("f")) for x in re.findall(r"-?[0-9.]+(?:e-?\d+)?f?", re.sub(r'"[^"]*"', "", line))]
        p = {"pos": f[0:3], "up": f[3:6], "look": f[6:9]}
        if bound:
            p["bbox"] = f[10:16]
        out.append(p)
    return out


class Setup:
    def __init__(self, ge: Ge, name: str) -> None:
        self.name = name
        src, self.path = ge.setup_c(name)
        self.pads = pads(src, "PadRecord padlist[] =", False)
        self.bpads = pads(src, "BoundPadRecord pad3dlist[] =", True)
        self.props = records(src, "s32 propDefs[] =")
        self.intro = records(src, "s32 intro[] =")

    def pad(self, n: int) -> dict | None:
        """Pad `n` as an object names it: >= 10000 a bound pad (`prop.c`)."""
        if 0 <= n < len(self.pads):
            return self.pads[n]
        if 10000 <= n < 10000 + len(self.bpads):
            return self.bpads[n - 10000]
        return None


def door_row(w: list[int], world: float) -> dict:
    """A GE door record's words as a PD `door()` row's fields (all but the
    model, pad and sibling). `world`: GE world units to PD centimetres."""
    flags = w[2] & ~(GE_DOOR_TWOWAY | GE_NO_PORTAL_CLOSE | GE_DOOR_KEEPOPEN)
    if w[2] & GE_DOOR_TWOWAY:
        flags |= PD_DOOR_TWOWAY
    if w[2] & GE_DOOR_KEEPOPEN:
        flags |= PD_DOOR_KEEPOPEN
    # GE's own multiplayer setups clear these (Ump_setuparkZ: every door's
    # flags2 is 0): no keys, no one-way locks, no airlocks (an airlock door
    # waits for its partner to shut, `doors_request_mode`, which a simulant
    # standing in the other one holds up for good).
    flags2 = w[3] & ~GE_FLAGS2_DROP & ~(FLAGS2_LOCKS | FLAGS2_AIRLOCK)
    return {
        "flags": flags, "flags2": flags2, "flags3": 0, "maxdamage": w[29] >> 16,
        "maxfrac": s32(w[33]), "perimfrac": s32(w[34]),
        # GE's accel and decel are / 65536 (`prop.c:1120`), PD's / 65536000
        # (`setup.c:1057`): the same rate in PD's units.
        "accel": s32(w[35]) * 1000, "decel": s32(w[36]) * 1000, "maxspeed": s32(w[37]),
        "doorflags": w[38] >> 16, "doortype": w[38] & 0xFFFF, "keyflags": 0,
        "autoclosetime": s32(w[40]),
        # PD's `unk88` is `xludist << 16 | opadist` (s16s, world units: PD's
        # `glass_calculate_opacity`, GE's `glassCalculateOpacity`);
        # `unkc4` is `soundtype << 8 | fadetime60`.
        "unk88": ((round(s32(w[48]) * world) & 0xFFFF) << 16) | (round(s32(w[49]) * world) & 0xFFFF),
        # SUBST: GE plays its own door samples / PD's bank has most of
        # them (GE_DOOR_SOUNDTYPE), all but the loops 211 and 216.
        "unkc4": GE_DOOR_SOUNDTYPE.get(w[41] & 0xFF, 0) << 8,
    }


def doors(setup: Setup, ge: Ge, k: float, world: float, pad_base: int) -> tuple[list[dict], list[dict], set[int], list[str]]:
    """The setup's doors as `setup.json` rows, with the pads they stand on
    (pads.json rows, numbered from `pad_base`). `k` takes the setup's (BG) units
    to PD centimetres, `world` GE's world units. Returns (pads, rows, the GE prop
    numbers of their models, report lines)."""
    out_pads: list[dict] = []
    pad_index: dict[int, int] = {}
    rows: list[dict] = []
    models: set[int] = set()
    cmd_of: dict[int, int] = {}  # setup record index -> row
    links: list[tuple[int, int]] = []
    unlocked = 0
    for idx, (ty, w) in enumerate(setup.props):
        if ty != "Door":
            continue
        obj, pad = w[1] >> 16, w[1] & 0xFFFF
        bp = setup.bpads[pad]
        if pad not in pad_index:
            pad_index[pad] = pad_base + len(out_pads)
            out_pads.append({"pos": [c * k for c in bp["pos"]], "look": bp["look"], "up": bp["up"],
                             "flags": PADFLAG_HASBBOXDATA, "bbox": [c * k for c in bp["bbox"]]})
        if (w[3] & ~GE_FLAGS2_DROP) & (FLAGS2_LOCKS | FLAGS2_AIRLOCK) or w[39]:
            unlocked += 1
        models.add(obj)
        cmd_of[idx] = len(rows)
        if s32(w[32]):
            links.append((len(rows), idx + s32(w[32])))
        rows.append({"type": "door", "scale": w[0] >> 16, "model": 0x1000 + obj, "pad": pad_index[pad], **door_row(w, world), "sibling": 0})
    for row, target in links:
        if target in cmd_of:
            rows[row]["sibling"] = cmd_of[target] - row  # relative, as PD's (setup.c:1063)
    kinds: dict[str, int] = {}
    for r in rows:
        kinds[f"{r['model']:#x}/type {r['doortype']}"] = kinds.get(f"{r['model']:#x}/type {r['doortype']}", 0) + 1
    report = [f"doors: {len(rows)} from {setup.name} on {len(out_pads)} pads ({unlocked} unlocked), "
              f"{sum(1 for r in rows if r['sibling'])} in sibling pairs; models {sorted(models)}"]
    return out_pads, rows, models, report


def markers(setup: Setup, k: float) -> list[dict]:
    """The spots a setup marks, for placement: its player starts, its guards,
    its pickups; armour, rarer, as a prize."""
    out = []

    def mark(kind: str, p: dict | None) -> None:
        if p is None:
            return
        look = p["look"]
        out.append({"kind": kind, "pos": [c * k for c in p["pos"]], "facing": math.atan2(look[0], look[2])})

    for ty, w in setup.intro:
        if ty == "Spawn" and len(w) > 1:
            mark("spawn", setup.pad(w[1]))
    for ty, w in setup.props:
        if len(w) < 2:
            continue
        pad = w[1] & 0xFFFF
        if ty == "Guard":
            mark("person", setup.pad(pad))
        elif ty in ("Collectable", "AmmoBox", "AmmoMag") and pad != 0xFFFF:
            mark("item", setup.pad(pad))
        elif ty == "Armour" and pad != 0xFFFF:
            mark("prize", setup.pad(pad))
    return out


def check_door_scale(setup: Setup) -> None:
    """`g_DoorScale` (a `DoorScale` record, `prop.c:980`) resizes doors in their
    frames; none is supported here."""
    for ty, w in setup.props:
        if ty == "DoorScale" and len(w) > 1 and w[1] != 0x10000:
            raise GeError(f"{setup.name}: a door scale of {w[1] / 65536} is not supported")


def _cross(u, v):
    return [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]


def _dot(u, v):
    return u[0] * v[0] + u[1] * v[1] + u[2] * v[2]


def _portal_normal(verts: list[list[float]]) -> list[float]:
    n = [0.0, 0.0, 0.0]
    for i in range(len(verts)):
        a, b = verts[i], verts[(i + 1) % len(verts)]
        n[0] += (a[1] - b[1]) * (a[2] + b[2])
        n[1] += (a[2] - b[2]) * (a[0] + b[0])
        n[2] += (a[0] - b[0]) * (a[1] + b[1])
    length = math.sqrt(_dot(n, n)) or 1.0
    return [c / length for c in n]


def align_portals(portals: list[dict], pads: list[dict], rows: list[dict]) -> int:
    """Move each door's portal onto the door's middle plane. Returns how many.

    GE puts a door in the rooms either side of it by construction (`setupDoor`,
    `prop.c:1141-1180`: its stan tile's room and the one across the pad's
    normal). PD finds a door's rooms from its box, through the portals it
    enters (`obj_detect_rooms`, `propobj.c:2850`), and GE's doors stand flush
    against their portals (one face on the portal's plane, some a few cm past
    it), so the room across the portal was missed, and a player on that side,
    who cannot see through the shut door's closed portal, could not use it
    (`current_player_interact` takes the doors in rooms on screen). The
    portal, found as PD finds a door's (`setup_get_portal_by_door_pad`,
    `setup.c:918`: the one the line along the pad's normal through its centre
    crosses), is moved the few cm along its own normal onto the door's
    middle."""
    moved = set()
    for r in rows:
        if not r["flags"] & 0x10000000:  # OBJFLAG_DOOR_HASPORTAL
            continue
        p = pads[r["pad"]]
        b, up, look = p["bbox"], p["up"], p["look"]
        n = _cross(up, look)
        centre = [p["pos"][k] + (b[0] + b[1]) / 2 * n[k] + (b[2] + b[3]) / 2 * up[k] + (b[4] + b[5]) / 2 * look[k] for k in range(3)]
        reach = (b[1] - b[0]) / 2 + 10.0
        best = None
        for i, portal in enumerate(portals):
            v = portal["verts"]
            pn = _portal_normal(v)
            denom = _dot(pn, n)
            if abs(denom) < 1e-6:
                continue
            # Where the line centre + t n meets the portal's plane, and whether
            # that point is inside its outline.
            t = _dot(pn, [v[0][k] - centre[k] for k in range(3)]) / denom
            if abs(t) > reach:
                continue
            q = [centre[k] + n[k] * t for k in range(3)]
            inside = all(_dot(pn, _cross([v[(j + 1) % len(v)][k] - v[j][k] for k in range(3)], [q[k] - v[j][k] for k in range(3)])) >= -1.0
                         for j in range(len(v))) or                      all(_dot(pn, _cross([v[(j + 1) % len(v)][k] - v[j][k] for k in range(3)], [q[k] - v[j][k] for k in range(3)])) <= 1.0
                         for j in range(len(v)))
            if inside and (best is None or abs(t) < abs(best[1])):
                best = (i, t, pn)
        if best is None or best[0] in moved:
            continue
        i, _, pn = best
        d = _dot(pn, [centre[k] - portals[i]["verts"][0][k] for k in range(3)])
        portals[i]["verts"] = [[v[k] + pn[k] * d for k in range(3)] for v in portals[i]["verts"]]
        moved.add(i)
    return len(moved)
