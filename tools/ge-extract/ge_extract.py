#!/usr/bin/env python3
"""Extract a GoldenEye 007 level into a PD Combat Simulator stage's files.

GoldenEye's levels are Perfect Dark's ancestors in every part PD's game runs
on (rooms and portals, floor tiles, pads, doors, the display lists, the image
format), so a GE level is converted into PD's own stage formats directly,
through PD's own exporters (`tools/pd-assets`: `pd_tex` decodes the images,
`pd_fpgun.Interp` runs the display lists, `pd_models.write_model` writes the
one model format). What the level lacks as an arena (waypoints, spawns,
weapons, hills, bases, cover) `pd_import` adds; it runs this first
(`crates/pd_import/src/ge.rs`), and this writes:

    <custom>/stages/<code>/bg.json + bg.bin   the BG (ge_bg), textures in <custom>/ge/tex/
                           tiles.json         the clipping, walls made (ge_stan)
                           ge.json            for pd_import: the doors' pads and rows,
                                              the spots the setups mark, the report
    <custom>/models/<stem>.json + .bin        the door models (ge_model),
                   index.json                 their rows (MODEL_* 0x1000 + GE prop number)
    <custom>/ge/tex/<nnnn>.png                the GE images drawn, by image number

Usage (pd_import passes these from the recipe's `source`):
    python tools/ge-extract/ge_extract.py --rom <rom> --decomp <ge-decomp> --level LEVELID_FACILITY
        --setup UsetuparkZ [--markers Ump_setuparkZ ...] --code facility --scale 1 --custom <custom>
    python tools/ge-extract/ge_extract.py --rom <rom> --decomp <ge-decomp> --custom <custom> --doors
        # every door model any setup places, and <custom>/ge/doors.json (ge_doors: the
        # level editor's catalogue of GoldenEye's doors)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import ge_bg  # noqa: E402
import ge_doors  # noqa: E402
import ge_model  # noqa: E402
import ge_setup  # noqa: E402
import ge_stan  # noqa: E402
from ge_rom import PD_ASSETS_TOOLS, Ge, GeError  # noqa: E402
from ge_tex import Images  # noqa: E402

sys.path.insert(0, PD_ASSETS_TOOLS)
import pd_models  # noqa: E402

EXPORTER = "tools/ge-extract/ge_extract.py"


def env_row(ge: Ge, levelid: str, code: str, world: float) -> dict:
    """The level's `fog_tables` row (`bgfog.c`, the NTSC table) as PD's
    `g_FogEnvironments` row: GE's columns are PD's (near, far, the distance
    fade's opa%, xlu% and reference distance, then `dif_ght` / `far_alight`
    are PD's fogmin / fogmax; NTSC adds `mnvisrng` and `intensity`, which PD
    dropped). Dam's row is PD's Pelagic's, to the digit."""
    text = ge._read("src", "game", "bgfog.c")
    ntsc = text[text.index("#else", text.index("EnvironmentRecord fog_tables")) :]
    m = re.search(rf"\{{\s*{levelid}\s*,([^}}]*)\}}", ntsc)
    if not m:
        raise GeError(f"{levelid} has no fog_tables row")
    v = [x.strip() for x in m.group(1).split(",")]
    num = lambda i: float(v[i]) if "." in v[i] else int(v[i], 0)  # noqa: E731
    near, far, opa, xlu, ref, _mnvis, _intensity, fogmin, fogmax, r, g, b, clouds, cloudscale = (num(i) for i in range(14))
    row = {
        "stage": f"STAGE_CUSTOM_{code.upper()}", "near": round(near * world), "far": round(far * world),
        "opaperc": opa, "xluperc": xlu, "refdist": round(ref * world), "fogmin": fogmin, "fogmax": fogmax,
        "sky_r": r, "sky_g": g, "sky_b": b, "numsuns": 0, "suns": None,
        # SUBST: GE's clouds and water (none on Facility) are not converted.
        "clouds_enabled": 0, "clouds_r": 255, "clouds_g": 255, "clouds_b": 255, "clouds_scale": cloudscale, "clouds_type": "TEX_ENV_00",
        "water_enabled": 0, "water_r": 0, "water_g": 0, "water_b": 0, "water_scale": 0, "water_type": "TEX_ENV_00",
        "clouds_height": 0,
    }
    if clouds:
        raise GeError(f"{levelid} has clouds, which are not converted yet")
    return {"fog": True, "transparency": False, "provenance": f"ge-decomp/src/game/bgfog.c fog_tables {levelid}",
            "replaced_commands": 0, "fogenvironment": row}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--rom", required=True)
    ap.add_argument("--decomp", required=True)
    ap.add_argument("--level", help="the levelinfotable row, e.g. LEVELID_FACILITY")
    ap.add_argument("--setup", help="the setup whose doors are kept, e.g. UsetuparkZ")
    ap.add_argument("--markers", nargs="*", default=[], help="more setups whose spots placement prefers")
    ap.add_argument("--code")
    ap.add_argument("--scale", type=float, default=1.0, help="GE world units (cm) to PD centimetres")
    ap.add_argument("--custom", required=True)
    ap.add_argument("--doors", action="store_true", help="every setup's door models and the door catalogue, no level")
    a = ap.parse_args()
    if not a.doors and not (a.level and a.setup and a.code):
        ap.error("--level, --setup and --code are required (or --doors)")

    try:
        ge = Ge(a.rom, a.decomp)
        if a.doors:
            for line in ge_doors.build(ge, Images(ge, a.custom), a.custom, a.scale):
                print(line)
            return 0
        level = ge.level(a.level)
        k = a.scale / level["levelscale"]
        stage = os.path.join(a.custom, "stages", a.code)
        os.makedirs(stage, exist_ok=True)
        images = Images(ge, a.custom)
        report = [f"GoldenEye {a.level}: {level['bg']}, {level['stan']}, level scale {level['levelscale']} "
                  f"(BG units per cm; x {a.scale} to PD cm)"]

        env = env_row(ge, a.level, a.code, a.scale)
        model, extra, rep = ge_bg.build(ge, images, level, k, a.code, env)
        if env["fog"]:
            # A fog stage's BG draws with G_RM_FOG_SHADE_A in cycle 1, as
            # GE's (and PD's) fog levels do (`Material.fog_shade`).
            for m in model["materials"]:
                m["fog_shade"] = True
        report += rep
        tops = {r["room"]: r["gfx_bbmax"][1] for r in extra["rooms"] if r["gfx_bbmax"]}
        tiles, rep = ge_stan.build(ge, level, k, tops, ge_stan.Solid(ge_bg.opaque_triangles(model)))
        with open(os.path.join(stage, "tiles.json"), "w", encoding="utf-8", newline="\n") as fh:
            json.dump(tiles, fh, separators=(",", ":"))
            fh.write("\n")
        report += rep

        setup = ge_setup.Setup(ge, a.setup)
        ge_setup.check_door_scale(setup)
        pads, rows, models, rep = ge_setup.doors(setup, ge, k, a.scale, 0)
        report += rep
        report.append(f"  {ge_setup.align_portals(extra['portals'], pads, rows)} portals moved onto their doors' middle planes")
        ge_bg.grow_rooms_to_portals(extra)
        pd_models.write_model(model, "bg", 0, "custom", stage, EXPORTER, extra)
        for propnum in sorted(models):
            stem, m, entry, warnings = ge_model.export(ge, images, propnum)
            ge_model.write(a.custom, stem, m, entry)
            report.append(f"  model {stem}: MODEL {entry['modelnum']:#x}, {entry['tris']} triangles"
                          + "".join(f"; WARN {w}" for w in warnings))
        marks = ge_setup.markers(setup, k)
        for name in a.markers:
            marks += ge_setup.markers(ge_setup.Setup(ge, name), k)
        report.append(f"markers: {len(marks)} ({', '.join(f'{n} {kind}' for kind, n in sorted(count(marks).items()))}) "
                      f"from {', '.join([a.setup, *a.markers])}")
        report.append(f"images: {len(images.written)} written to ge/tex/")
        out = {"format": "pd-ge-extract/1", "exporter": EXPORTER, "pads": pads, "props": rows, "markers": marks, "report": report}
        with open(os.path.join(stage, "ge.json"), "w", encoding="utf-8", newline="\n") as fh:
            json.dump(out, fh, separators=(",", ":"))
            fh.write("\n")
    except GeError as e:
        print(f"ge_extract: {e}", file=sys.stderr)
        return 1
    for line in report:
        print(line)
    return 0


def count(marks: list[dict]) -> dict[str, int]:
    out: dict[str, int] = {}
    for m in marks:
        out[m["kind"]] = out.get(m["kind"], 0) + 1
    return out


if __name__ == "__main__":
    sys.exit(main())
