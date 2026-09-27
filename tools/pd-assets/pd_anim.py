#!/usr/bin/env python3
"""Perfect Dark animation (.bin) decoder.

Decodes the bit-packed per-bone rotation/translation channels PD stores for each
animation, so a character model can be posed. Ported from `lib/anim.c`
(`anim_get_rot_translate_scale`, `anim_read_bits`) in the decompilation.

# File layout

`struct animtableentry` (types.h:5017) describes each animation:

    u16 numframes; u16 bytesperframe; u32 data; u16 headerlen; u8 framelen; u8 flags

and the extracted `.bin` is exactly `[header: headerlen bytes][frame 0][frame 1]...`
with each frame `bytesperframe` long. `animations.json` carries the same fields under
the extractor's names: `unk08` is `headerlen` and `unk0a` is `framelen`.

# Header

A flat sequence of per-part records, one per animated bone, in part-number order.
Each starts with a flags byte (`ANIMFIELD_*`, constants.h:296) selecting which
channels that part animates, followed by a descriptor per channel. A descriptor
carries a **base value** and a **bit length**; the frame data then holds only a
small bit-packed delta added to that base. That is the whole compression scheme,
and it is why `bytesperframe` is as low as 11 for a 163-frame animation.

Walking the header also accumulates `bitoffset` — where in each frame's bit stream
that part's data begins.

# Rotations

Stored as Euler XYZ. The packed integer is shifted left by `16 - framelen` (so
`framelen` is the retained precision, typically 12 bits) and scaled by
`BADDTOR(360) / 65536` into radians. Rare's pi is used, as everywhere else.

Usage:
    python pd_anim.py list [substring]
    python pd_anim.py info <ANIM_ID or index>
    python pd_anim.py frame <ANIM_ID or index> <framenum>
"""

from __future__ import annotations

import argparse
import json
import math
import os
import struct
import sys

# Rare's pi, matching pd_model.py and include/math.h:5.
BAD_PI = 3.141_092_6
# introt -> radians (anim.c:539).
ROT_SCALE = (BAD_PI * 2.0) / 65536.0

# ANIMFIELD_* (constants.h:296)
F_S16_ROTATE = 0x01
F_S16_TRANSLATE = 0x02
F_08 = 0x08
F_F32_ROTATE = 0x10
F_S32_TRANSLATE = 0x20
F_CAMERA = 0x40
F_F32_SCALE = 0x80

ASSETS = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..",
    "..",
    "reference",
    "pd-decomp",
    "src",
    "assets",
    "ntsc-final",
)


def read_bits(data: bytes, numbits: int, bitoffset: int) -> int:
    """`anim_read_bits` (anim.c:374) — big-endian bit extraction."""
    if numbits <= 0:
        return 0
    result = 0
    pos = bitoffset // 8
    bitoffset %= 8
    numbitsthisbyte = 8 - bitoffset
    remaining = numbits
    while remaining >= numbitsthisbyte:
        if pos >= len(data):
            return result
        remaining -= numbitsthisbyte
        mask = (1 << numbitsthisbyte) - 1
        result |= (data[pos] & mask) << remaining
        pos += 1
        numbitsthisbyte = 8
    if remaining > 0 and pos < len(data):
        mask = (1 << remaining) - 1
        result |= (data[pos] >> (numbitsthisbyte - remaining)) & mask
    return result


def read_signed_short(data: bytes, numbits: int, bitoffset: int) -> int:
    """`anim_read_signed_short` (anim.c:407) — sign-extend to 16 bits."""
    result = read_bits(data, numbits, bitoffset)
    # A 0-bit channel is a constant: the base is the whole value and there is
    # nothing in the frame to sign-extend. (`read_bits` returns 0, so the C sign
    # test is false either way; Python just refuses to shift by -1.)
    if 0 < numbits < 16 and (result & (1 << (numbits - 1))):
        result |= ((1 << (16 - numbits)) - 1) << numbits
    return result & 0xFFFF


def s16(v: int) -> int:
    return v - 0x10000 if v & 0x8000 else v


class PartChannels:
    """One part's header record: which channels it animates and where its bits are."""

    __slots__ = (
        "flags", "bitoffset", "translate", "motion", "rotate", "rot_is_f32", "trans_is_s32",
    )

    def __init__(self) -> None:
        self.flags = 0
        self.bitoffset = 0
        # each entry: (base, bitlen)
        self.translate: list[tuple[int, int]] = []
        #: `ANIMFIELD_08` — **root motion**: four channels `(x, y, z, angle)` read by
        #: `anim_get_pos_angle_as_int` (anim.c:610), not by
        #: `anim_get_rot_translate_scale`, which zeroes `translate` for such a part.
        #: This is how PD moves a character *through* an animation: how far a walk
        #: cycle strides, and how far a death falls.
        #:
        #: Its bits sit in each frame **before** the part's rotation bits, so it has
        #: to be stepped over even when the motion itself is not wanted. See
        #: [`Animation.part_transform`].
        self.motion: list[tuple[int, int]] = []
        self.rotate: list[tuple[int, int]] = []
        self.rot_is_f32 = False
        self.trans_is_s32 = False


class Animation:
    def __init__(self, meta: dict, data: bytes):
        self.id: str = meta["id"]
        self.numframes: int = meta["numframes"]
        self.bytesperframe: int = meta["bytesperframe"]
        self.headerlen: int = meta["unk08"]
        self.framelen: int = meta["unk0a"]
        self.data = data
        self.header = data[: self.headerlen]
        self.parts: list[PartChannels] = self._parse_header()

    # -- header ------------------------------------------------------------

    def _parse_header(self) -> list[PartChannels]:
        """Walk the per-part records, recording each part's channels and the bit
        offset at which its data starts within a frame.

        This mirrors the skip-loop at the top of `anim_get_rot_translate_scale`
        (anim.c:445) and the read block below it, but does it once for every part
        instead of re-walking from part 0 on every query.
        """
        parts: list[PartChannels] = []
        h = self.header
        p = 0
        bitoffset = 0
        while p < len(h):
            pc = PartChannels()
            pc.bitoffset = bitoffset
            flags = h[p]
            pc.flags = flags
            p += 1

            # ── translation ──
            if flags & F_S16_TRANSLATE:
                for k in range(3):
                    if p + 3 > len(h):
                        return parts
                    base = (h[p] << 8) + h[p + 1]
                    bits = h[p + 2]
                    pc.translate.append((base, bits))
                    bitoffset += bits
                    p += 3
            elif flags & F_S32_TRANSLATE:
                pc.trans_is_s32 = True
                for k in range(3):
                    if p + 5 > len(h):
                        return parts
                    bits = h[p]
                    base = (h[p + 1] << 24) | (h[p + 2] << 16) | (h[p + 3] << 8) | h[p + 4]
                    pc.translate.append((base, bits))
                    bitoffset += bits
                    p += 5
            elif flags & F_08:
                # Root motion: four `(base, bitlen)` channels — x, y, z, angle — laid
                # out exactly like the S16 translate ones, read by
                # `anim_get_pos_angle_as_int` (anim.c:672-690). Its bits are part of
                # this part's frame data and PRECEDE the rotation bits, so they are
                # recorded rather than merely skipped: `part_transform` has to step
                # over them to find the rotation, and `part_motion` reads them.
                if p + 12 > len(h):
                    return parts
                for k in range(3):
                    pc.motion.append(((h[p] << 8) + h[p + 1], h[p + 2]))
                    bitoffset += h[p + 2]
                    p += 3
                # The fourth channel is a facing angle, in the same s16 encoding.
                pc.motion.append(((h[p] << 8) + h[p + 1], h[p + 2]))
                bitoffset += h[p + 2]
                p += 3

            # ── rotation ──
            if flags & F_S16_ROTATE:
                for k in range(3):
                    if p + 3 > len(h):
                        return parts
                    base = (h[p] << 8) + h[p + 1]
                    bits = h[p + 2]
                    pc.rotate.append((base, bits))
                    bitoffset += bits
                    p += 3
            elif flags & F_F32_ROTATE:
                pc.rot_is_f32 = True
                bitoffset += 96  # three raw f32s in the frame data, nothing in the header

            if flags & F_CAMERA:
                if p + 5 > len(h):
                    return parts
                bitoffset += h[p]
                p += 5

            if flags & F_F32_SCALE:
                bitoffset += 0x60

            parts.append(pc)
        return parts

    # -- frames ------------------------------------------------------------

    def frame_bytes(self, framenum: int) -> bytes:
        """`anim_load_frame` (anim.c:307): header, then fixed-size frames."""
        if self.bytesperframe == 0:
            return b""
        start = self.headerlen + self.bytesperframe * framenum
        return self.data[start : start + self.bytesperframe]

    def part_transform(self, part: int, framenum: int):
        """Return `(rot_xyz_radians, translate_xyz)` for `part` at `framenum`.

        Missing channels come back as zeros, which is what the engine does — a
        part with no rotation channel simply keeps its rest orientation.
        """
        if part >= len(self.parts):
            return (0.0, 0.0, 0.0), (0.0, 0.0, 0.0)
        pc = self.parts[part]
        fb = self.frame_bytes(framenum)
        bit = pc.bitoffset
        # A root-motion part's four channels occupy the first bits of its frame
        # data, ahead of the rotation — `anim_get_rot_translate_scale` steps over
        # them (anim.c:510) before reading the angles, and so must we. Skipping this
        # reads the rotation out of the motion channels' bits, which yields a
        # *coherent but wrongly-oriented* body: the pose is fine, the whole figure is
        # simply lying down. It hid because the only clip anyone had rendered,
        # `ANIM_TWO_GUN_HOLD`, is authored dead still — all four of its bit lengths
        # are 0, so the misalignment was exactly zero on that one animation.
        for _base, bits in pc.motion:
            bit += bits

        translate = [0.0, 0.0, 0.0]
        if pc.translate:
            for i, (base, bits) in enumerate(pc.translate):
                if pc.trans_is_s32:
                    raw = read_bits(fb, bits, bit)
                    # The base is a raw s32 pattern; combine then scale (anim.c:500).
                    val = (raw + base) & 0xFFFFFFFF
                    if val & 0x8000_0000:
                        val -= 0x1_0000_0000
                    translate[i] = val * 0.001
                else:
                    raw = read_signed_short(fb, bits, bit)
                    translate[i] = float(s16((raw + base) & 0xFFFF))
                bit += bits

        rot = [0.0, 0.0, 0.0]
        if pc.rot_is_f32:
            for i in range(3):
                raw = read_bits(fb, 32, bit)
                rot[i] = struct.unpack(">f", raw.to_bytes(4, "big"))[0]
                bit += 32
        elif pc.rotate:
            shift = 16 - self.framelen
            for i, (base, bits) in enumerate(pc.rotate):
                raw = read_bits(fb, bits, bit)
                introt = ((raw + base) & 0xFFFF) << shift
                introt &= 0xFFFF
                rot[i] = introt * ROT_SCALE
                bit += bits

        return tuple(rot), tuple(translate)

    def part_motion(self, part: int, framenum: int):
        """Root motion for `part` at `framenum`: `(x, y, z, angle_radians)`.

        `anim_get_pos_angle_as_int` (anim.c:610) — the `ANIMFIELD_08` channels,
        which `part_transform` deliberately reports as no translation because the
        game does the same (`anim_get_rot_translate_scale` zeroes `translate` for
        such a part). This is where a clip's travel actually lives: how far a walk
        cycle strides, and how far — and how far *down* — a death falls.

        `(0, 0, 0, 0)` for a part with no motion channel, so a caller can treat
        every part uniformly.
        """
        if part >= len(self.parts):
            return 0.0, 0.0, 0.0, 0.0
        pc = self.parts[part]
        if not pc.motion:
            return 0.0, 0.0, 0.0, 0.0
        fb = self.frame_bytes(framenum)
        bit = pc.bitoffset
        out = []
        for base, bits in pc.motion:
            raw = read_signed_short(fb, bits, bit)
            out.append(float(s16((raw + base) & 0xFFFF)))
            bit += bits
        # The fourth channel is a 16-bit turn: `angle * BADDTOR(360) / 65536`
        # (anim.c:706), the same fixed-point turn the rotations use.
        return out[0], out[1], out[2], out[3] * ROT_SCALE


# ---------------------------------------------------------------------------
# Catalogue
# ---------------------------------------------------------------------------


def load_manifest() -> list[dict]:
    with open(os.path.join(ASSETS, "animations.json"), encoding="utf-8") as fh:
        return json.load(fh)


def load_animation(key: str) -> Animation:
    manifest = load_manifest()
    meta = None
    if key.isdigit():
        meta = manifest[int(key)]
    else:
        want = key.upper()
        for m in manifest:
            if m["id"].upper() == want:
                meta = m
                break
    if meta is None:
        raise SystemExit(f"no animation matching {key!r}")
    with open(os.path.join(ASSETS, "animations", meta["file"]), "rb") as fh:
        return Animation(meta, fh.read())


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------


def cmd_list(substring: str | None) -> int:
    for i, m in enumerate(load_manifest()):
        if substring and substring.upper() not in m["id"].upper():
            continue
        print(
            f"{i:5d}  {m['id']:<34} frames={m['numframes']:<5} "
            f"bytes/frame={m['bytesperframe']:<4} headerlen={m['unk08']:<5} framelen={m['unk0a']}"
        )
    return 0


def cmd_info(key: str) -> int:
    a = load_animation(key)
    print(f"{a.id}: {a.numframes} frames, {a.bytesperframe} bytes/frame")
    print(f"  headerlen {a.headerlen}, framelen {a.framelen} bits")
    print(f"  parsed {len(a.parts)} animated parts")
    expected = a.headerlen + a.bytesperframe * a.numframes
    print(f"  file {len(a.data)} bytes, expected {expected}", "OK" if len(a.data) == expected else "MISMATCH")
    rot = sum(1 for p in a.parts if p.rotate or p.rot_is_f32)
    tra = sum(1 for p in a.parts if p.translate)
    mot = sum(1 for p in a.parts if p.motion)
    print(f"  parts with rotation: {rot}, with translation: {tra}, with root motion: {mot}")
    for i, p in enumerate(a.parts):
        if p.motion:
            bits = [b for _, b in p.motion]
            end = a.part_motion(i, a.numframes - 1)
            print(
                f"    part {i} root motion: bitlens {bits}, "
                f"last frame (x={end[0]:.0f} y={end[1]:.0f} z={end[2]:.0f} "
                f"angle={math.degrees(end[3]):.1f}deg)"
            )
    # Every part's bits, i.e. one whole frame. Should account for `bytesperframe`
    # up to the byte the encoder rounds up to; a large shortfall means a channel is
    # going unread and every part after it is being decoded from the wrong bits.
    last = a.parts[-1] if a.parts else None
    total_bits = 0
    if last is not None:
        total_bits = last.bitoffset
        for _b, n in last.motion + last.translate + last.rotate:
            total_bits += n
    print(f"  frame uses {total_bits} bits ({total_bits / 8:.1f} of {a.bytesperframe} bytes)")
    return 0


def cmd_frame(key: str, framenum: int) -> int:
    a = load_animation(key)
    print(f"{a.id} frame {framenum}/{a.numframes}")
    for i, _ in enumerate(a.parts):
        rot, tra = a.part_transform(i, framenum)
        deg = tuple(math.degrees(r) for r in rot)
        print(
            f"  part {i:3d}  rot=({deg[0]:8.2f},{deg[1]:8.2f},{deg[2]:8.2f})deg  "
            f"tra=({tra[0]:9.2f},{tra[1]:9.2f},{tra[2]:9.2f})"
        )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("list", help="list animations")
    p.add_argument("substring", nargs="?")

    p = sub.add_parser("info", help="header summary for one animation")
    p.add_argument("anim")

    p = sub.add_parser("frame", help="dump every part's transform at a frame")
    p.add_argument("anim")
    p.add_argument("framenum", type=int)

    args = ap.parse_args()
    if args.cmd == "list":
        return cmd_list(args.substring)
    if args.cmd == "info":
        return cmd_info(args.anim)
    return cmd_frame(args.anim, args.framenum)


if __name__ == "__main__":
    sys.exit(main())
