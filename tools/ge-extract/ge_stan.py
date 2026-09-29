"""GoldenEye's clipping ("stan", `Tbg_<level>_all_p_stanZ`) into PD's `tiles.json`.

A GE stan tile is PD's floor tile (PD's `geotilei` descends from it): an
outline of up to eight s16 points, a room (the BG's room number), and the
floor's shade in the same nibbles PD's `floorcol` reads (`stan.c:2834`;
PD `propobj.c:1623`). What GE does not have is walls: its tiles are floors
only, each edge linked to the tile across it, and an edge with no link is
where you cannot go (`stan.c:492, 596`). PD's collision is polygons, walls
among them, so the walls are made here, one for every unlinked edge:

* nothing beyond the edge (the level's shell), a tile beyond at the same
  height (a partition), or only higher tiles (the face under a raised floor):
  a wall with PD's arena flags, `WALL | BLOCK_SIGHT | BLOCK_SHOOT` (0x1c), from
  just below the floor up to the top of the room's geometry, or, if lower,
  to a step below the next floor above the edge (so a wall never cuts
  through the storey above, nor into the feet of someone standing a step
  down from it, as on a stair running over a landing);
* a lower tile beyond and nothing drawn across the edge at head height (a
  railing, a parapet or an open ledge over a drop): `WALL` alone, so nobody
  walks off it (GE's rule) and nobody's view or shot across the drop is
  stopped by a wall that isn't drawn. Whether something is drawn there is the
  BG's own triangles: a short ray out over the edge at 1.5 m (a lower room
  behind a solid wall is a wall).

GE's stairs have a tile for every riser, standing on its edge (no floor area:
walking up them is GE's walk over tiles, which crosses it at once). PD kept
them when it remade this level as Felicity: its risers are the same vertical
tiles, floors flagged `GEOFLAG_STEP` (0x201b, `assets/stages/mp11`), which
`cd_find_ground_finalise` takes as a step. So a riser is written so, and makes
no wall (its only unlinked edges are its vertical ends).

The file (ge-decomp `src/game/stan.h`, `bondtypes.h:421`): a 12-byte header
(`{u32 0, u32 first tile, u8 pad[4]}`), then the tiles to an all-zero one:
`u32 id << 8 | room`, `u16 special << 12 | r << 8 | g << 4 | b`, `u16 npoints
<< 12 | the three plane point indices`, then `npoints x {s16 x, y, z; u16
link}`; link `>> 4 == 0` is no neighbour. Coordinates are BG units (world x
levelscale, `stan.c:331`).
"""

from __future__ import annotations

import math
import struct

from ge_rom import Ge, GeError

#: PD's `GEOFLAG_*` (pd-decomp `constants.h:1189`).
GEOFLAG_FLOOR1, GEOFLAG_FLOOR2, GEOFLAG_WALL = 0x1, 0x2, 0x4
GEOFLAG_BLOCK_SIGHT, GEOFLAG_BLOCK_SHOOT = 0x8, 0x10
GEOFLAG_LADDER, GEOFLAG_AIBOTCROUCH, GEOFLAG_STEP = 0x40, 0x800, 0x2000
FLOOR = GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2 | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT  # 0x1b, the arenas' floors
WALL = GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT  # 0x1c, the arenas' walls

#: `g_StanTileSpecialFlags` (`stan.c:64`) by a tile's `special`: 1 forces a
#: crouch (the vents, `stan.c:2327`), 3 is a ladder.
SPECIAL_CROUCH, SPECIAL_LADDER = 1, 3

#: How far past an edge to look for the tile beyond (cm), and what height
#: difference makes it another level (cm).
PROBE = 8.0
LEVEL = 30.0
#: How far below the floor a made wall starts (cm): covers sloped floors.
WALL_FOOT = 20.0
#: A made wall reaches at least this far above its floor (cm).
WALL_MIN = 250.0
#: A made wall under a floor ends this far below it (cm): a stair's rise (26
#: cm on Facility) and the 20 cm a chr's cylinder stands off its floor.
WALL_UNDER = 60.0


class Tile:
    __slots__ = ("room", "special", "col", "pts", "links", "plane")

    def __init__(self, room, special, col, pts, links, plane):
        self.room = room
        self.special = special
        self.col = col
        self.pts = pts
        self.links = links
        self.plane = plane

    def y_at(self, x: float, z: float) -> float:
        """The floor's height at (x, z), on the plane through its three plane
        points (as `stan.c` finds a tile's height)."""
        a, b, c = (self.pts[i] for i in self.plane)
        n = ((b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
             (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
             (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
        if abs(n[1]) < 1e-9:
            return max(p[1] for p in self.pts)
        return a[1] - (n[0] * (x - a[0]) + n[2] * (z - a[2])) / n[1]

    def is_riser(self) -> bool:
        """Standing on its edge: its outline covers (almost) no floor."""
        n = len(self.pts)
        flat = abs(sum(self.pts[j][0] * self.pts[(j + 1) % n][2] - self.pts[(j + 1) % n][0] * self.pts[j][2] for j in range(n))) / 2
        cx = cy = cz = 0.0
        for j in range(1, n - 1):
            a, b, c = self.pts[0], self.pts[j], self.pts[j + 1]
            u = (b[0] - a[0], b[1] - a[1], b[2] - a[2])
            v = (c[0] - a[0], c[1] - a[1], c[2] - a[2])
            cx += u[1] * v[2] - u[2] * v[1]
            cy += u[2] * v[0] - u[0] * v[2]
            cz += u[0] * v[1] - u[1] * v[0]
        full = math.sqrt(cx * cx + cy * cy + cz * cz) / 2
        return full > 0 and flat < 0.05 * full

    def contains(self, x: float, z: float) -> bool:
        inside = False
        n = len(self.pts)
        for i in range(n):
            (x0, _, z0), (x1, _, z1) = self.pts[i], self.pts[(i + 1) % n]
            if (z0 > z) != (z1 > z) and x < x0 + (z - z0) * (x1 - x0) / (z1 - z0):
                inside = not inside
        return inside


def parse(data: bytes, k: float) -> list[Tile]:
    (first,) = struct.unpack_from(">I", data, 4)
    o = first if 0 < first < len(data) else 12
    tiles = []
    while o + 8 <= len(data):
        idroom, mid, tail = struct.unpack_from(">IHH", data, o)
        n = tail >> 12
        if n == 0:
            break
        pts, links = [], []
        for i in range(n):
            x, y, z, link = struct.unpack_from(">hhhH", data, o + 8 + 8 * i)
            pts.append((x * k, y * k, z * k))
            links.append(link >> 4 != 0)
        plane = ((tail >> 8) & 0xF, (tail >> 4) & 0xF, tail & 0xF)
        if max(plane) >= n:
            plane = (0, 1, 2)
        tiles.append(Tile(idroom & 0xFF, mid >> 12, mid & 0x0FFF, pts, links, plane))
        o += 8 + 8 * n
    else:
        raise GeError("the stan file has no terminating tile")
    return tiles


class Solid:
    """The BG's opaque triangles (cm), for a ray's first hit."""

    CELL = 200.0

    def __init__(self, tris: list[tuple[tuple[float, float, float], ...]]) -> None:
        self.tris = tris
        self.grid: dict[tuple[int, int], list[int]] = {}
        for i, t in enumerate(tris):
            xs = [p[0] for p in t]
            zs = [p[2] for p in t]
            for cx in range(math.floor(min(xs) / self.CELL), math.floor(max(xs) / self.CELL) + 1):
                for cz in range(math.floor(min(zs) / self.CELL), math.floor(max(zs) / self.CELL) + 1):
                    self.grid.setdefault((cx, cz), []).append(i)

    def hits(self, o: tuple[float, float, float], d: tuple[float, float, float], length: float) -> bool:
        """Does the segment `o + t d`, `0 <= t <= length`, meet a triangle
        (Moller-Trumbore, either side)?"""
        cells = set()
        for f in (0.0, 0.5, 1.0):
            cells.add((math.floor((o[0] + d[0] * length * f) / self.CELL), math.floor((o[2] + d[2] * length * f) / self.CELL)))
        seen = set()
        for c in cells:
            for i in self.grid.get(c, []):
                if i in seen:
                    continue
                seen.add(i)
                a, b, cc = self.tris[i]
                e1 = (b[0] - a[0], b[1] - a[1], b[2] - a[2])
                e2 = (cc[0] - a[0], cc[1] - a[1], cc[2] - a[2])
                pv = (d[1] * e2[2] - d[2] * e2[1], d[2] * e2[0] - d[0] * e2[2], d[0] * e2[1] - d[1] * e2[0])
                det = e1[0] * pv[0] + e1[1] * pv[1] + e1[2] * pv[2]
                if abs(det) < 1e-9:
                    continue
                tv = (o[0] - a[0], o[1] - a[1], o[2] - a[2])
                u = (tv[0] * pv[0] + tv[1] * pv[1] + tv[2] * pv[2]) / det
                if u < 0 or u > 1:
                    continue
                qv = (tv[1] * e1[2] - tv[2] * e1[1], tv[2] * e1[0] - tv[0] * e1[2], tv[0] * e1[1] - tv[1] * e1[0])
                v = (d[0] * qv[0] + d[1] * qv[1] + d[2] * qv[2]) / det
                if v < 0 or u + v > 1:
                    continue
                t = (e2[0] * qv[0] + e2[1] * qv[1] + e2[2] * qv[2]) / det
                if 0 <= t <= length:
                    return True
        return False


#: How high over an edge's floor a drawn surface makes it a wall (cm), and how
#: far out over the edge the ray looks (from a little inside it).
SOLID_HEIGHT = 150.0
SOLID_REACH = 40.0


def build(ge: Ge, level: dict, k: float, room_tops: dict[int, float], solid: Solid) -> tuple[dict, list[str]]:
    """`tiles.json` (pd-tiles/1) and report lines. `room_tops`: each room's
    highest drawn point (cm), where its made walls end; `solid`: the BG's
    opaque triangles, which tell a wall from an open drop."""
    tiles = parse(ge.file(level["stan"]), k)
    # The tiles over each 4 m cell, for the look past an edge.
    cell = 400.0
    grid: dict[tuple[int, int], list[int]] = {}
    for i, t in enumerate(tiles):
        xs = [p[0] for p in t.pts]
        zs = [p[2] for p in t.pts]
        for cx in range(math.floor(min(xs) / cell), math.floor(max(xs) / cell) + 1):
            for cz in range(math.floor(min(zs) / cell), math.floor(max(zs) / cell) + 1):
                grid.setdefault((cx, cz), []).append(i)

    def beyond(x: float, z: float, skip: int) -> list[float]:
        out = []
        for j in grid.get((math.floor(x / cell), math.floor(z / cell)), []):
            if j != skip and not tiles[j].is_riser() and tiles[j].contains(x, z):
                out.append(tiles[j].y_at(x, z))
        return out

    #: A floor this far above an edge is the storey above it (cm).
    storey = 100.0

    out = []
    counts = {"floors": 0, "walls": 0, "rails": 0, "crouch": 0, "ladders": 0, "risers": 0}
    for i, t in enumerate(tiles):
        if t.is_riser():
            counts["risers"] += 1
            out.append({"room": t.room, "flags": FLOOR | GEOFLAG_STEP, "floortype": 0, "floorcol": t.col,
                        "verts": [list(p) for p in t.pts]})
            continue
        flags = FLOOR
        if t.special == SPECIAL_CROUCH:
            # SUBST: GE forces the player to crouch on these (the vents,
            # `stan.c:2327`) / PD has no such floor: simulants crouch
            # (`GEOFLAG_AIBOTCROUCH`), a player crouches himself.
            flags |= GEOFLAG_AIBOTCROUCH
            counts["crouch"] += 1
        elif t.special == SPECIAL_LADDER:
            flags |= GEOFLAG_LADDER
            counts["ladders"] += 1
        out.append({"room": t.room, "flags": flags, "floortype": 0, "floorcol": t.col, "verts": [list(p) for p in t.pts]})
        counts["floors"] += 1
        # Which way is out: the polygon's winding in XZ.
        area = sum(t.pts[j][0] * t.pts[(j + 1) % len(t.pts)][2] - t.pts[(j + 1) % len(t.pts)][0] * t.pts[j][2] for j in range(len(t.pts)))
        for e, linked in enumerate(t.links):
            if linked:
                continue
            a, b = t.pts[e], t.pts[(e + 1) % len(t.pts)]
            dx, dz = b[0] - a[0], b[2] - a[2]
            length = math.hypot(dx, dz)
            if length < 1.0:
                continue
            # The outward normal: right of the edge for an anticlockwise
            # (positive area) outline in (x, z).
            nx, nz = (dz / length, -dx / length) if area > 0 else (-dz / length, dx / length)
            ey = (a[1] + b[1]) / 2
            ys, above = [], []
            for f in (0.25, 0.5, 0.75):
                px, pz = a[0] + dx * f + nx * PROBE, a[2] + dz * f + nz * PROBE
                ys += beyond(px, pz, i)
                # The storey above, on either side of the edge.
                qx, qz = a[0] + dx * f - nx * PROBE, a[2] + dz * f - nz * PROBE
                above += [y for y in beyond(px, pz, i) + beyond(qx, qz, i) if y > ey + storey]
            lower = [y for y in ys if y < ey - LEVEL]
            level_or_higher = [y for y in ys if ey - LEVEL <= y <= ey + storey]
            top = max(room_tops.get(t.room, ey + WALL_MIN), max(a[1], b[1]) + WALL_MIN)
            if above:
                top = min(top, min(above) - WALL_UNDER)
            lo = min(a[1], b[1]) - WALL_FOOT
            wflags = WALL
            drawn = any(solid.hits((a[0] + dx * f - nx * 10.0, ey + SOLID_HEIGHT, a[2] + dz * f - nz * 10.0), (nx, 0.0, nz), SOLID_REACH)
                        for f in (0.25, 0.5, 0.75))
            if lower and not level_or_higher and not drawn:
                wflags = GEOFLAG_WALL
                counts["rails"] += 1
            else:
                counts["walls"] += 1
            out.append({"room": t.room, "flags": wflags, "floortype": 0, "floorcol": 0,
                        "verts": [[a[0], lo, a[2]], [b[0], lo, b[2]], [b[0], top, b[2]], [a[0], top, a[2]]]})
    out.sort(key=lambda r: r["room"])  # stable: a room's tiles keep the file's order
    rooms = sorted({t["room"] for t in out})
    report = [f"clipping: {counts['floors']} floor tiles ({counts['crouch']} crouch, {counts['ladders']} ladder) "
              f"and {counts['risers']} stair risers (GEOFLAG_STEP), "
              f"{counts['walls']} walls and {counts['rails']} drop rails made at their unlinked edges, {len(rooms)} rooms"]
    return {"format": "pd-tiles/1", "source": f"ge007.u {level['stan']} (ROM)", "exporter": "tools/ge-extract/ge_stan.py",
            "rooms": rooms, "tiles": out}, report
