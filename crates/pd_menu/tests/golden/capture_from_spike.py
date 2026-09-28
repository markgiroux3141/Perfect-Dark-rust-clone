#!/usr/bin/env python3
"""Capture the menu goldens from the old repo's spike (M2, run once).

Runs the old `pd_combat_sim_snapshot` over every line of scripts.txt and stores
each shot at PD's 320x220 in this directory. The old tool writes 3x
nearest-neighbour PNGs of the RGBA5551-quantised framebuffer, so taking every
third pixel recovers the frame exactly.

Build the old tool first, into a scratch target so the old repo is untouched:

    cd "<old repo>/native"
    CARGO_TARGET_DIR=<scratch> cargo build --release --locked -p game --bin pd_combat_sim_snapshot

then:

    python crates/pd_menu/tests/golden/capture_from_spike.py <scratch>/release/pd_combat_sim_snapshot.exe

Needs Pillow.
"""

from __future__ import annotations

import os
import shlex
import subprocess
import sys
import tempfile

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
SCALE = 3


def runs() -> list[list[str]]:
    out = []
    with open(os.path.join(HERE, "scripts.txt"), encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#"):
                out.append(shlex.split(line))
    return out


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    exe = sys.argv[1]
    names = set()
    with tempfile.TemporaryDirectory() as tmp:
        for args in runs():
            res = subprocess.run([exe, tmp, *args], capture_output=True, text=True)
            if res.returncode != 0:
                print(f"failed: {' '.join(args)}\n{res.stderr}", file=sys.stderr)
                return 1
            for path in res.stdout.split():
                name = os.path.splitext(os.path.basename(path))[0]
                if name in names:
                    print(f"duplicate shot {name}", file=sys.stderr)
                    return 1
                names.add(name)
                big = Image.open(path).convert("RGB")
                w, h = big.width // SCALE, big.height // SCALE
                small = Image.new("RGB", (w, h))
                src, dst = big.load(), small.load()
                for y in range(h):
                    for x in range(w):
                        dst[x, y] = src[x * SCALE, y * SCALE]
                small.save(os.path.join(HERE, f"{name}.png"), optimize=True)
                print(name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
