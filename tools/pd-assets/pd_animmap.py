"""Measure the GoldenEye <-> Perfect Dark animation-id correspondence.

Perfect Dark and GoldenEye share **one animation bank**. Rare carried the GE
character animations into PD at the *same numbers*, so the GE clip the game ships
as `2A-jogging.glb` is PD's `ANIM_002A`. Everything the hunter clip set does with
PD animations rests on that, so it is measured here rather than assumed — run this
and read the table.

Two independent signals, either of which could have been a coincidence alone:

1. **Frame count.** Each GE clip GLB's key count against the `numframes` of the PD
   animation with the same id. 26 of the 36 clips the hunter template loads match
   exactly — 143, 227, 245, 185 frames and so on. Frame counts that specific do not
   collide by accident 26 times.

2. **What the body does.** `pd_triage.py`'s posture curve over the clip, GE's
   version on a GE body against PD's on a PD body. `21-death-forward-face-down-hard`
   goes 1.80 -> 0.34 m on GoldenEye's Karl; `ANIM_DEATH_0021` goes 2.07 -> 0.38 m on
   PD's A51 guard, turn for turn, at the 1.15x height ratio between the two rigs.

The 10 that do NOT match frame counts are the interesting half:

* `00-idle` -> `ANIM_0001` (ANIM_TWO_GUN_HOLD), not `ANIM_0000`. PD's slot 0 is a
  null entry with no frames, so the two banks are off by one at exactly that index
  and nowhere else.
* `01-fire-standing` -> `ANIM_0032`, which is a member of PD's own
  `g_StandHeavyAttackAnims` (`game/chraction.c:956`) and matches at 106 frames.
* `16`, `18`, `1B`, `1D`, `1E`, `1F` — six GE deaths whose PD same-id animations are
  something else entirely. `pd_triage.py` shows all six PD clips stay on their feet
  for their whole length, so they are not deaths at all. `pd_roster.json` fills
  those six slots from `g_DeathAnimsHuman*` instead.
* `1D`/`1E`/`1F` are also *mirrors* of `1C`/`1B`/`20` in the GE set (identical frame
  counts within the set). PD does not ship mirrors: `struct animtablerow` carries a
  `flip` flag and the game mirrors at runtime, which is why GE has clips PD lacks.

Usage:
    python pd_animmap.py            # the whole table
    python pd_animmap.py --check    # exit 1 if a claimed match stops matching
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import struct
import sys

import pd_anim

GE_DIR = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..", "..", "native", "assets", "enemies", "animations",
)

#: GE clip id -> the PD animation that is NOT the same number, and why. Everything
#: absent from here is expected to be `ANIM_%04X` of the same id.
EXCEPTIONS = {
    0x00: ("ANIM_TWO_GUN_HOLD", "PD slot 0 is a null entry, so idle sits at 0x01"),
    0x01: ("ANIM_0032", "PD's standing heavy attack (g_StandHeavyAttackAnims)"),
    0x16: (None, "PD's ANIM_0016 never leaves its feet - not a death"),
    0x18: (None, "PD's ANIM_0018 never leaves its feet - not a death"),
    0x1B: (None, "PD's ANIM_001B never leaves its feet - not a death"),
    0x1D: (None, "PD's ANIM_001D never leaves its feet - not a death (GE mirror of 1C)"),
    0x1E: (None, "PD's ANIM_001E never leaves its feet - not a death (GE mirror of 1B)"),
    0x1F: (None, "PD's ANIM_001F never leaves its feet - not a death (GE mirror of 20)"),
}


def glb_json(path: str) -> dict:
    data = open(path, "rb").read()
    off, out = 12, {}
    while off < len(data):
        ln, ty = struct.unpack_from("<II", data, off)
        if ty == 0x4E4F534A:
            out = json.loads(data[off + 8 : off + 8 + ln])
        off += 8 + ln
    return out


def ge_key_count(path: str) -> int:
    """How many keyframes a GoldenEye clip GLB holds (its first sampler's input)."""
    g = glb_json(path)
    sampler = g["animations"][0]["samplers"][0]
    return g["accessors"][sampler["input"]]["count"]


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--check", action="store_true", help="exit 1 if a claimed match breaks")
    args = ap.parse_args()

    manifest = pd_anim.load_manifest()
    by_index = {i: m for i, m in enumerate(manifest)}
    by_id = {m["id"]: (i, m) for i, m in enumerate(manifest)}

    matched = mismatched = 0
    print(f"{'GoldenEye clip':<44}{'keys':>6}  {'Perfect Dark':<26}{'frames':>7}")
    for path in sorted(glob.glob(os.path.join(GE_DIR, "*.glb"))):
        name = os.path.basename(path)
        hexid = int(name[:2], 16)
        keys = ge_key_count(path)

        if hexid in EXCEPTIONS:
            pd_id, why = EXCEPTIONS[hexid]
            if pd_id is None:
                print(f"{name:<44}{keys:>6}  {'(no counterpart)':<26}{'':>7}  {why}")
                continue
            idx, meta = by_id[pd_id]
            tag = "ok " if meta["numframes"] == keys else "!! "
        else:
            meta = by_index.get(hexid)
            pd_id = meta["id"] if meta else "(missing)"
            tag = "ok " if meta and meta["numframes"] == keys else "!! "
            why = ""

        frames = meta["numframes"] if meta else 0
        if tag == "ok ":
            matched += 1
        else:
            mismatched += 1
        print(f"{tag}{name:<41}{keys:>6}  {pd_id:<26}{frames:>7}  {why}")

    print(f"\n{matched} matched, {mismatched} unexpectedly mismatched, "
          f"{len(EXCEPTIONS) - 2} GE-only clips with no PD counterpart")
    if args.check and mismatched:
        print("FAIL: an id this file claims is shared no longer matches", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
