"""Posture screen for Perfect Dark animations — the cheap first pass before looking.

`pd_preview.py` frames every tile independently *on purpose* (so a limb flung out
of shot still fills the picture), which makes a contact sheet the wrong instrument
for one specific question: **does this clip end with the character on the ground?**
Posture is exactly what per-tile auto-framing normalises away, and that question is
what separates a death from an injury from a locomotion cycle when all you have is
an id like `ANIM_024E`.

So: skin the body at N times across a clip and measure its **standing height** —
the vertical extent of the posed mesh, in metres. A standing human is ~1.7 m tall;
the same human face-down is ~0.4 m. The shape of that curve over the clip names the
clip's kind without anyone guessing from a number:

    stays tall                  locomotion / idle / a standing fire
    tall, dips, returns tall    an injury — a flinch that recovers
    tall, then short, stays     a death

This is a **screen, not a verdict** — it says which of 1,207 animations are worth
rendering, and `pd_preview.py` still has to be pointed at the survivors. It shares
the glTF/skinning implementation with `pd_preview.py` (imported, not re-derived),
so it inherits that renderer's independence from the exporter.

Usage:
    python pd_triage.py <clip.glb> [<clip.glb> ...] [--body <model.glb>] [--samples 12]
"""

from __future__ import annotations

import argparse
import os
import sys

import pd_preview

# A human PD body is ~1.73 m. Below this fraction of its own standing height the
# figure is off its feet rather than merely crouching — measured against the known
# death set (`g_DeathAnimsHuman*`), which all settle at 0.20–0.35.
DOWN_FRAC = 0.45

DEFAULT_BODY = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..", "..", "native", "assets", "enemies", "pd", "characters", "pd_a51guard.glb",
)


def heights(model: pd_preview.Model, clip: pd_preview.Clip, samples: int) -> list[float]:
    """Posed vertical extent (metres) at `samples` times evenly across the clip."""
    out = []
    for i in range(samples):
        t = clip.duration * i / max(samples - 1, 1)
        pos = model.skin_positions(model.joint_matrices(clip.locals(model, t)))
        lo = min(p[1] for p in pos)
        hi = max(p[1] for p in pos)
        # Model units are millimetres (see `pd_pose.py`'s UNITS_PER_METRE).
        out.append((hi - lo) / 1000.0)
    return out


def classify(hs: list[float]) -> str:
    """Name the clip's kind from its standing-height curve. See the module docs."""
    tall = max(hs)
    if not tall:
        return "empty"
    start, end = hs[0] / tall, hs[-1] / tall
    lowest = min(hs) / tall
    if end < DOWN_FRAC:
        return "DEATH  (ends on the ground)"
    if lowest < DOWN_FRAC and start > DOWN_FRAC and end > DOWN_FRAC:
        return "injury (goes down, gets up)"
    if tall - min(hs) < 0.15:
        return "upright (locomotion / fire / idle)"
    return "upright, but dips"


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("clips", nargs="+")
    ap.add_argument("--body", default=DEFAULT_BODY)
    ap.add_argument("--samples", type=int, default=12)
    args = ap.parse_args()

    model = pd_preview.Model(args.body)
    for path in args.clips:
        clip = pd_preview.Clip(path, model)
        hs = heights(model, clip, args.samples)
        curve = " ".join(f"{h:.2f}" for h in hs)
        name = os.path.splitext(os.path.basename(path))[0]
        print(f"{name:<24} {clip.duration:>5.2f}s  {curve}   -> {classify(hs)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
