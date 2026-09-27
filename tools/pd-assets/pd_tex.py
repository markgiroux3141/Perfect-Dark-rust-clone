#!/usr/bin/env python3
"""Perfect Dark texture decoder — the compressed global pool.

Character models reference textures two ways. The four bodies that store them
*inline* are handled in `pd_gltf.py` (already-decompressed `RGBA5551`). Everything
else — 62 of the 66 bodies and **all 76 head models** — indexes the global pool in
`textures/`, whose files are compressed. This module decodes those.

Ported from `game/texdecompress.c` + `game/texreset.c` (`tex_read_bits`) and
`lib/rzip.s`.

# File layout

    [flags byte][payload]

The flags byte (`tex_load`, `texdecompress.c:2141`):

    bit 7   hasloddata   — the payload carries its own mip images
    bit 6   iszlib       — which inflate path
    bits 0-5 numlods     — clamped to 5

## The `iszlib` path (2,886 of 3,502 non-empty textures; 0x0d42 is empty)

A bit stream (MSB-first, `tex_read_bits`), which in practice stays byte-aligned:

    u8  format        — TEXFORMAT_* (constants.h:4349)
    u8  numcolours-1
    u16 palette[numcolours]
    per image:
        u8 width, u8 height
        an rzip stream: 0x11 0x73, u24 uncompressed length, then raw DEFLATE

Every texture on this path is **paletted** — 2,462 `RGBA16_CI4`, 417
`RGBA16_CI8`, 7 `IA16_CI8` — which is why `tex_align_indices` only ever handles
the CI cases (its `indicesperbyte` is left uninitialised for the others).

**The inflated data is linear.** `tex_swizzle` and the 8-byte row padding are
applied *after* inflation, to put the image into the layout the RDP reads — so
unlike the inline textures, nothing here needs unswizzling. Getting that backwards
costs you a fine vertical comb over every face; see `HANDOFF_PD_ASSETS.md` bug 7.

## The non-`iszlib` path (616 textures)

`tex_inflate_non_zlib` (`texdecompress.c:699`) — a different codec entirely,
and **never paletted**: 181 `I8`, 150 `IA8`, 125 `I4`, 86 `RGBA32`, 35 `IA4`,
25 `RGBA16`, 10 `RGB24`, 4 `RGB15` at level 0. One bit stream, not byte-aligned,
with a 24-bit header per image (`:736`):

    u4 format  u8 width  u8 height  u4 method (TEXCOMPMETHOD_*, constants.h:4338)

and each image picks its own format and method. The methods (`:756-826`) are
built from four parts, all ported literally:

* **channels** — the image is split into planes (`g_TexFormatNumChannels`, `:53`;
  a 1-bit alpha is a separate plane read *after* the others, at `3*w*h` whatever
  the channel count, `:765`), coded by `HUFFMAN` (one table for all planes),
  `HUFFMANPERHCHANNEL`, `RLE`, or those two followed by `tex_blur` (`:1991`),
  which is a predictive un-filter (left/above/diagonal/Paeth-ish, 3-bit method).
* **Huffman** (`:1192`) stores only `chansize` 8-bit frequencies; the tree is
  rebuilt by a quirky merge whose tie-breaks *are* the code assignment, so it is
  a transliteration, u16 frequencies and 9999 sentinel included.
* **RLE** (`:1350`) is LZ77-like: 3/3/4-bit field sizes, then literal/run
  directives, and a run is always followed by one unmarked literal.
* **lookup** (`:1421`) — an 11-bit count and a table of full texels; indices are
  packed at `ceil(log2 n)` bits (`LOOKUP`), or Huffman/RLE-coded (`HUFFMANLOOKUP`,
  `RLELOOKUP`). A one-entry table costs 0 bits per pixel: a constant image.

As on the zlib path, the result is linear; `tex_swizzle` runs afterwards.

Between images the reader byte-aligns, and **a reader already on a boundary
skips a whole byte** (`:835`). Six textures (0x14c, 0x150, 0x90d, 0x91a, 0x91b,
0xa8f) store 7 LODs while `tex_load` clamps `numlods` to 5 (`:2208`); their
streams decode cleanly to the byte when all 7 are walked. `check` walks every
stored image and requires the cursor to land exactly on the end of the file —
Huffman and RLE carry no lengths, so a wrong tree cannot land there by luck.
All 616 do, except the two below.

**Two textures are broken in the game itself.** 0x114 and 0x88e (`I8`,
`HUFFMAN`) have frequency sums of 17,360 and 19,003. The game's tree builder
treats any node weight >= 9999 as consumed, so it stops early and decodes with
a truncated tree — 350 and 922 bytes of their data are never read. We decode
what the N64 decodes. Simply lifting the limit does not recover the intended
image either (228 / 628 bytes still unread), so the encoder differed in some
other way too. Neither is named in `textureconfig.c` or used by an exported gun.

### Texel -> RGBA8

What the RDP does: 4-bit channels expand by replication (`*17`), IA4's 3-bit
intensity as `i<<5 | i<<2 | i>>1`, 5-bit via [`rgba5551`], and **`I4`/`I8` put
the intensity in alpha too** (an I texel is I,I,I,I). A consumer that treats
alpha as coverage on an opaque surface must ignore it for I formats.

### Checked against the editor dump (`pd dump/weapons`, see `pd_gltf.read_bmp`)

61 non-zlib textures are in both. Raw RGBA: 32 identical. The editor differs
from the RDP on three conventions — it expands 4/3-bit channels by *shifting*
(`n<<4`, `i<<5`), gives I8 alpha 255, and pads the width of 8-bit images to 8
texels (0x296 is 56 wide there, 54 here; 0x606 8 vs 4, and textureconfig
agrees with us). With those three mapped, **57 of 61 match every pixel**
(all 18 I8, 5 IA4, 2 IA8, 32 of 33 RGBA32). The other four are the editor's:
0x3cd (RGBA32, all-black alpha mask) has identical RGB but is fully transparent
in the dump; and all 3 `RGB24` are mangled there — 0x3d3 has R and B swapped
(near-grey, so it is off by <= 2), 0x3de/0x409 have a 255 rotating through the
channels, i.e. 4-byte texels read as 3-byte ones.

`RGB24` texels read straight off the stream (`LOOKUP`) get alpha 0 (`:1731`, no
`| 0xff`); that is the game's own quirk, kept.

Usage:
    python pd_tex.py info  <texture.bin> [...]
    python pd_tex.py check [-v]                 # decode all 3,503, summarise
    python pd_tex.py png <outdir> <hexnum> [...] [--manifest x.json]
    python pd_tex.py sheet <out.png> <texture.bin> [...]
"""

from __future__ import annotations

import argparse
import os
import re
import struct
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# TEXFORMAT_* (constants.h:4349)
FMT_RGBA32, FMT_RGBA16, FMT_RGB24, FMT_RGB15 = 0, 1, 2, 3
FMT_IA16, FMT_IA8, FMT_IA4, FMT_I8, FMT_I4 = 4, 5, 6, 7, 8
FMT_RGBA16_CI8, FMT_RGBA16_CI4, FMT_IA16_CI8, FMT_IA16_CI4 = 9, 10, 11, 12

FORMAT_NAMES = {
    FMT_RGBA32: "RGBA32", FMT_RGBA16: "RGBA16", FMT_RGB24: "RGB24", FMT_RGB15: "RGB15",
    FMT_IA16: "IA16", FMT_IA8: "IA8", FMT_IA4: "IA4", FMT_I8: "I8", FMT_I4: "I4",
    FMT_RGBA16_CI8: "RGBA16_CI8", FMT_RGBA16_CI4: "RGBA16_CI4",
    FMT_IA16_CI8: "IA16_CI8", FMT_IA16_CI4: "IA16_CI4",
}

#: Palette-index width, per `tex_align_indices`.
INDICES_PER_BYTE = {
    FMT_RGBA16_CI8: 1, FMT_IA16_CI8: 1,
    FMT_RGBA16_CI4: 2, FMT_IA16_CI4: 2,
}
#: Which formats read their 16-bit palette entries as IA16 rather than RGBA5551.
IA_PALETTE = {FMT_IA16_CI8, FMT_IA16_CI4}


class UnsupportedTexture(Exception):
    """A texture this module cannot decode yet (the non-zlib codec)."""


class BitReader:
    """`tex_read_bits` (`texreset.c:21`) — MSB-first over a byte string."""

    def __init__(self, data: bytes):
        self.data = data
        self.pos = 0
        self.acc = 0
        self.nbits = 0

    def read(self, want: int) -> int:
        while self.nbits < want:
            if self.pos >= len(self.data):
                raise UnsupportedTexture("bit stream ran out")
            self.acc = (self.acc << 8) | self.data[self.pos]
            self.pos += 1
            self.nbits += 8
        self.nbits -= want
        return (self.acc >> self.nbits) & ((1 << want) - 1)

    @property
    def byte_pos(self) -> int:
        """Next unread byte. Only meaningful while the stream is byte-aligned,
        which it is for the whole header (8/8/16/8/8-bit fields)."""
        if self.nbits % 8:
            raise UnsupportedTexture("bit stream is not byte-aligned")
        return self.pos - self.nbits // 8


def rzip_inflate(data: bytes, off: int) -> tuple[bytes, int]:
    """Inflate one PD rzip stream at `off`; returns `(bytes, next offset)`.

    `lib/rzip.s:223`: PD's format is `0x11 0x73`, then a 3-byte uncompressed
    length, then raw DEFLATE. (GoldenEye's `0x11 0x72` omits the length.) The
    consumed length is recovered from the decompressor rather than stored, which
    is what lets the caller walk on to the next mip image.
    """
    if data[off] != 0x11 or data[off + 1] != 0x73:
        raise UnsupportedTexture(
            f"expected an rzip 1173 stream at {off}, found {data[off]:#04x} {data[off+1]:#04x}"
        )
    outlen = (data[off + 2] << 16) | (data[off + 3] << 8) | data[off + 4]
    d = zlib.decompressobj(-15)  # raw DEFLATE, no zlib/gzip wrapper
    out = d.decompress(data[off + 5 :], outlen)
    if len(out) < outlen:
        raise UnsupportedTexture(f"rzip stream produced {len(out)} of {outlen} bytes")
    consumed = len(data) - off - 5 - len(d.unused_data)
    return out[:outlen], off + 5 + consumed


def rgba5551(v: int) -> tuple[int, int, int, int]:
    """N64 `RGBA16`: `rrrrrgggggbbbbba`. The 5-bit channels scale by `*255//31`
    so full scale stays full scale — `<<3` would cap white at 248 and grey the
    whole texture down."""
    return (
        ((v >> 11) & 31) * 255 // 31,
        ((v >> 6) & 31) * 255 // 31,
        ((v >> 1) & 31) * 255 // 31,
        255 if v & 1 else 0,
    )


def ia16(v: int) -> tuple[int, int, int, int]:
    """N64 `IA16`: 8 bits intensity, 8 bits alpha."""
    i = (v >> 8) & 0xFF
    return (i, i, i, v & 0xFF)


class PoolTexture:
    """One decoded pool texture: level 0 as RGBA8, plus what it was."""

    __slots__ = ("width", "height", "format", "rgba", "numlods", "hasloddata", "numcolours")

    def __init__(self, width, height, fmt, rgba, numlods, hasloddata, numcolours):
        self.width = width
        self.height = height
        self.format = fmt
        self.rgba = rgba
        self.numlods = numlods
        self.hasloddata = hasloddata
        self.numcolours = numcolours

    @property
    def format_name(self) -> str:
        return FORMAT_NAMES.get(self.format, f"?{self.format}")


# ---------------------------------------------------------------------------
# The non-zlib codec: `tex_inflate_non_zlib` (texdecompress.c:699-891)

#: TEXCOMPMETHOD_* (constants.h:4338).
COMP_UNCOMPRESSED0, COMP_UNCOMPRESSED1, COMP_HUFFMAN, COMP_HUFFMANPERCHANNEL = 0, 1, 2, 3
COMP_RLE, COMP_LOOKUP, COMP_HUFFMANLOOKUP, COMP_RLELOOKUP = 4, 5, 6, 7
COMP_HUFFMANBLUR, COMP_RLEBLUR = 8, 9

COMP_NAMES = {
    COMP_UNCOMPRESSED0: "UNCOMPRESSED0", COMP_UNCOMPRESSED1: "UNCOMPRESSED1",
    COMP_HUFFMAN: "HUFFMAN", COMP_HUFFMANPERCHANNEL: "HUFFMANPERCHANNEL",
    COMP_RLE: "RLE", COMP_LOOKUP: "LOOKUP", COMP_HUFFMANLOOKUP: "HUFFMANLOOKUP",
    COMP_RLELOOKUP: "RLELOOKUP", COMP_HUFFMANBLUR: "HUFFMANBLUR", COMP_RLEBLUR: "RLEBLUR",
}

#: Per-format tables, indexed by TEXFORMAT_* (texdecompress.c:53-63).
#: Channels, *excluding* a 1-bit alpha channel.
FORMAT_NUM_CHANNELS = (4, 3, 3, 3, 2, 2, 1, 1, 1, 1, 1, 1, 1)
#: Whether the format carries a separate 1-bit alpha plane.
FORMAT_HAS_1BIT_ALPHA = (0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0)
#: Number of distinct values per channel (so 32 = 5 bits). This is also the
#: Huffman alphabet size for the per-channel methods.
FORMAT_CHANNEL_SIZES = (256, 32, 256, 32, 256, 16, 8, 256, 16, 256, 16, 256, 16)
#: Bits per lookup-table entry for the LOOKUP methods.
FORMAT_BITS_PER_PIXEL = (32, 16, 24, 15, 16, 8, 4, 8, 4, 16, 16, 16, 16)

#: Guard bytes past the end of a file. The game DMAs `(len + 0x1f) >> 4 << 4`
#: bytes (texdecompress.c:2198), so a reader that strays a little past the
#: data sees padding, not a fault. We allow the same slack (as zeros).
_PAD_BITS = 32 * 8

_BIT_TABLE = [
    bytes((b >> 7 & 1, b >> 6 & 1, b >> 5 & 1, b >> 4 & 1, b >> 3 & 1, b >> 2 & 1, b >> 1 & 1, b & 1))
    for b in range(256)
]


class BitString:
    """`tex_read_bits` (texreset.c:21) as a bit position.

    The C reader keeps an accumulator plus `g_TexAccumNumBits`, which is always
    < 8 after a read, so the pair is exactly a bit cursor: `pos*8 - numbits`.
    Expanding the payload to one byte per bit makes the Huffman walk cheap.
    """

    __slots__ = ("bits", "pos", "limit", "notes")

    def __init__(self, data: bytes):
        self.bits = b"".join(_BIT_TABLE[b] for b in data) + bytes(_PAD_BITS)
        self.limit = len(data) * 8
        self.pos = 0
        #: Non-fatal oddities met while decoding (see `_huffman`).
        self.notes: list[str] = []

    def read(self, n: int) -> int:
        p = self.pos
        e = p + n
        if e > len(self.bits):
            raise UnsupportedTexture("bit stream ran out")
        v = 0
        for b in self.bits[p:e]:
            v = (v << 1) | b
        self.pos = e
        return v

    def end_image(self) -> None:
        """The inter-image step at texdecompress.c:835: a reader sitting exactly
        on a byte boundary (`g_TexAccumNumBits == 0`) skips a WHOLE byte, and one
        mid-byte discards the rest of it. Only matters from LOD 1 on."""
        if self.pos % 8 == 0:
            self.pos += 8
        else:
            self.pos = (self.pos + 7) & ~7


def _huffman(bs: BitString, count: int, chansize: int) -> list[int]:
    """`tex_inflate_huffman` (texdecompress.c:1192).

    Rare stores only the `chansize` 8-bit frequencies; the tree is rebuilt by a
    quirky merge whose tie-breaks decide the codes, so it is ported literally,
    including the u16 frequencies and the 9999 "consumed" sentinel. Branch
    values >= 10000 are leaves (value + 10000); smaller ones are node slots,
    which reuse the slots of consumed leaves.
    """
    if chansize < 2:
        # With one symbol the C leaves `minindex1` uninitialised.
        raise UnsupportedTexture(f"huffman over a {chansize}-symbol alphabet")
    freq = [bs.read(8) for _ in range(chansize)]
    n0 = [-1] * chansize
    n1 = [-1] * chansize

    def two_smallest():
        # texdecompress.c:1217-1233 and :1279-1295 (the same rule, written twice).
        f1 = f2 = 9999
        i1 = i2 = -1
        for i, f in enumerate(freq):
            if f < f1:
                if f2 < f1:
                    f1, i1 = f, i
                else:
                    f2, i2 = f, i
            elif f < f2:
                f2, i2 = f, i
        return f1, i1, f2, i2

    _f1, m1, _f2, m2 = two_smallest()
    root = -1
    subtrees = chansize  # to spot a tree cut short by the 9999 sentinel
    while True:
        subtrees -= 1
        s = freq[m1] + freq[m2]
        if s == 0:
            s = 1
        s &= 0xFFFF  # frequencies[] is u16
        freq[m1] = 9999
        freq[m2] = 9999
        if n0[m1] < 0 and n1[m1] < 0:
            n0[m1] = m1 + 10000
            root = m1
            freq[m1] = s
            n1[m1] = m2 + 10000 if (n0[m2] < 0 and n1[m2] < 0) else m2
        elif n0[m2] < 0 and n1[m2] < 0:
            n0[m2] = m2 + 10000
            root = m2
            freq[m2] = s
            n1[m2] = m1 + 10000 if (n0[m1] < 0 and n1[m1] < 0) else m1
        else:
            root = 0
            while n0[root] >= 0 or n1[root] >= 0 or freq[root] < 9999:
                root += 1
                if root >= chansize:
                    # The C walks on into uninitialised stack.
                    raise UnsupportedTexture("huffman ran out of node slots")
            freq[root] = s
            n0[root] = m1
            n1[root] = m2
        f1, m1, f2, m2 = two_smallest()
        if f1 == 9999 or f2 == 9999:
            break
    if subtrees > 1:
        # A node weighing >= 9999 looks consumed, so the build stopped with
        # several subtrees and only the last one is reachable. The game decodes
        # with it anyway, and so do we (0x114 and 0x88e).
        bs.notes.append(f"huffman tree truncated by the 9999 sentinel ({subtrees} subtrees)")

    # Walk the tree per symbol (texdecompress.c:1304).
    nodes = [0] * (chansize * 2)
    nodes[0::2] = n0
    nodes[1::2] = n1
    bits = bs.bits
    p = bs.pos
    end = len(bits)
    out = [0] * count
    for i in range(count):
        v = root
        while v < 10000:
            if p >= end:
                raise UnsupportedTexture("bit stream ran out")
            v = nodes[v * 2 + bits[p]]
            p += 1
        out[i] = v - 10000
    bs.pos = p
    return out


def _rle(bs: BitString, total: int) -> tuple[list[int], int]:
    """`tex_inflate_rle` (texdecompress.c:1350) — an LZ77-ish block stream.

    Header: 3-bit backtrack field size, 3-bit run-length field size, 4-bit block
    size. Then `0 literal` or `1 back len` directives; a run is always followed by
    one bare literal (no marker bit), and run lengths are biased by `fudge`, the
    number of literals a run must beat to be worth encoding. Returns the blocks
    and the block size (> 8 means the C stored them as u16).
    """
    btsize = bs.read(3)
    rlsize = bs.read(3)
    blocksize = bs.read(4)
    cost = btsize + rlsize + blocksize + 1
    fudge = 0
    while cost > 0:
        cost -= blocksize + 1
        fudge += 1
    out: list[int] = []
    while len(out) < total:
        if bs.read(1) == 0:
            out.append(bs.read(blocksize))
        else:
            start = len(out) - bs.read(btsize) - 1
            run = bs.read(rlsize) + fudge
            if start < 0:
                raise UnsupportedTexture("RLE backtrack before the start")
            for i in range(start, start + run):
                out.append(out[i])  # may overlap what it is writing, as in C
            out.append(bs.read(blocksize))
    # The C can overshoot `blockstotal` by a run; the tail is never read.
    return out, blocksize


def _build_lookup(bs: BitString, bpp: int) -> list[int]:
    """`tex_build_lookup` (texdecompress.c:1421): 11-bit count, then entries of
    `g_TexFormatBitsPerPixel` bits (32-bit ones read as 24 + 8)."""
    n = bs.read(11)
    if bpp <= 24:
        return [bs.read(bpp) for _ in range(n)]
    return [(bs.read(24) << 8) | bs.read(bpp - 24) for _ in range(n)]


def _bit_size(n: int) -> int:
    """`tex_get_bit_size` (texdecompress.c:1449): bits to index `n` values, so a
    one-entry table costs 0 bits per pixel — a constant image."""
    n -= 1
    c = 0
    while n > 0:
        n >>= 1
        c += 1
    return c


def _half(v: int) -> int:
    """C's `v / 2`, which truncates toward zero."""
    return -((-v) // 2) if v < 0 else v // 2


def _blur(px: list[int], width: int, height: int, method: int, chansize: int) -> None:
    """`tex_blur` (texdecompress.c:1991) — really a predictive *un*-filter, in
    place and in raster order, so each prediction uses already-restored
    neighbours. `height` is `channels * height`: the planes are filtered as one
    tall image. Method 7 is a no-op (the switch has no case for it)."""
    if method > 6:
        return
    for y in range(height):
        row = y * width
        for x in range(width):
            i = row + x
            cur = px[i] + chansize * 2
            left = px[i - 1] if x > 0 else 0
            above = px[i - width] if y > 0 else 0
            al = px[i - width - 1] if (x > 0 and y > 0) else 0
            if method == 0:
                v = cur + left
            elif method == 1:
                v = cur + above
            elif method == 2:
                v = cur + al
            elif method == 3:
                v = cur + (left + above - al)
            elif method == 4:
                v = cur + (_half(above - al) + left)
            elif method == 5:
                v = cur + (_half(left - al) + above)
            else:
                v = cur + _half(left + above)
            px[i] = (v % chansize) & 0xFF  # pixels[] is u8


def _channels_to_texels(s: list[int], width: int, height: int, fmt: int) -> list[int]:
    """`tex_channels_to_pixels` (texdecompress.c:1564), minus the row padding.

    `s` is planar: channel k at `k * w*h`, and the 1-bit alpha plane — whatever
    the channel count — at `3 * w*h` (texdecompress.c:765). The result is one
    N64 texel value per pixel, in raster order. The C packs 4-bit formats two per
    byte with a `pos--` fix-up for odd widths, which is exactly a raster walk.
    """
    m = width * height
    rng = range(m)
    if fmt == FMT_RGBA32:
        return [s[p] << 24 | s[p + m] << 16 | s[p + 2 * m] << 8 | s[p + 3 * m] for p in rng]
    if fmt == FMT_RGB24:
        return [s[p] << 24 | s[p + m] << 16 | s[p + 2 * m] << 8 | 0xFF for p in rng]
    if fmt == FMT_RGBA16:
        return [(s[p] << 11 | s[p + m] << 6 | s[p + 2 * m] << 1 | s[p + 3 * m]) & 0xFFFF for p in rng]
    if fmt == FMT_IA16:
        return [(s[p] << 8 | s[p + m]) & 0xFFFF for p in rng]
    if fmt == FMT_RGB15:
        return [(s[p] << 11 | s[p + m] << 6 | s[p + 2 * m] << 1 | 1) & 0xFFFF for p in rng]
    if fmt == FMT_IA8:
        return [(s[p] << 4 | s[p + m]) & 0xFF for p in rng]
    if fmt == FMT_I8:
        return [s[p] & 0xFF for p in rng]
    if fmt == FMT_IA4:
        # texdecompress.c:1659: `i << 5 | a << 4 | i' << 1 | a'` per byte.
        return [(s[p] << 1 | s[p + 3 * m]) & 0xF for p in rng]
    if fmt == FMT_I4:
        return [s[p] & 0xF for p in rng]
    # The C has no case for the CI formats and returns 0 bytes.
    raise UnsupportedTexture(f"non-zlib channel methods have no {FORMAT_NAMES.get(fmt, fmt)} case")


def _lookup_texels(idx: list[int], width: int, height: int, lut: list[int], fmt: int,
                   from_buffer: bool) -> list[int]:
    """`tex_inflate_lookup` (texdecompress.c:1706) and
    `tex_inflate_lookup_from_buffer` (:1799): index -> table entry.

    Two faithful quirks: an `RGB24` entry read straight off the bit string gets
    alpha 0 (`<< 8` with no `| 0xff`, :1731) while the buffered variant ORs in
    0xff (:1838); and the 4-bit formats build each byte as
    `(L[a] << 4 | L[b]) & 0xff`, so the pairing is emulated at byte level.
    """
    n = len(lut)
    if idx and max(idx) >= n:
        raise UnsupportedTexture(f"lookup index {max(idx)} outside a {n}-entry table")
    if fmt == FMT_RGBA32:
        return [lut[i] & 0xFFFFFFFF for i in idx]
    if fmt == FMT_RGB24:
        a = 0xFF if from_buffer else 0
        return [((lut[i] << 8) | a) & 0xFFFFFFFF for i in idx]
    if fmt in (FMT_RGBA16, FMT_IA16):
        return [lut[i] & 0xFFFF for i in idx]
    if fmt == FMT_RGB15:
        return [((lut[i] << 1) | 1) & 0xFFFF for i in idx]
    if fmt in (FMT_IA8, FMT_I8):
        return [lut[i] & 0xFF for i in idx]
    if fmt in (FMT_IA4, FMT_I4):
        out = [0] * (width * height)
        for y in range(height):
            row = y * width
            for x in range(0, width, 2):
                a = lut[idx[row + x]]
                if x + 1 < width:
                    b = lut[idx[row + x + 1]]
                elif from_buffer and row + x + 1 < len(idx):
                    b = lut[idx[row + x + 1]]  # reads the next row's first index
                else:
                    b = 0
                byte = ((a << 4) | b) & 0xFF
                out[row + x] = byte >> 4
                if x + 1 < width:
                    out[row + x + 1] = byte & 0xF
        return out
    raise UnsupportedTexture(f"lookup methods have no {FORMAT_NAMES.get(fmt, fmt)} case")


def _read_uncompressed(bs: BitString, width: int, height: int, fmt: int) -> list[int]:
    """`tex_read_uncompressed` (texdecompress.c:1478): raw texels, row by row.
    4-bit formats read a byte per pixel *pair*, so an odd-width row carries a
    padding nibble."""
    m = width * height
    if fmt == FMT_RGBA32:
        return [(bs.read(16) << 16) | bs.read(16) for _ in range(m)]
    if fmt == FMT_RGB24:
        return [(bs.read(24) << 8) | 0xFF for _ in range(m)]
    if fmt in (FMT_RGBA16, FMT_IA16):
        return [bs.read(16) for _ in range(m)]
    if fmt == FMT_RGB15:
        return [(bs.read(15) << 1) | 1 for _ in range(m)]
    if fmt in (FMT_IA8, FMT_I8):
        return [bs.read(8) for _ in range(m)]
    if fmt in (FMT_IA4, FMT_I4):
        out = [0] * m
        for y in range(height):
            row = y * width
            for x in range(0, width, 2):
                b = bs.read(8)
                out[row + x] = b >> 4
                if x + 1 < width:
                    out[row + x + 1] = b & 0xF
        return out
    raise UnsupportedTexture(f"uncompressed path has no {FORMAT_NAMES.get(fmt, fmt)} case")


def _inflate_image(bs: BitString) -> tuple[int, int, int, int, list[int]]:
    """One image of `tex_inflate_non_zlib` (texdecompress.c:736-826).

    Header `ffff wwwwwwww hhhhhhhh cccc` (format, width, height, method). Returns
    `(format, width, height, method, texels)` with texels in raster order.
    """
    fmt = bs.read(4)
    width = bs.read(8)
    height = bs.read(8)
    comp = bs.read(4)
    if fmt > FMT_IA16_CI4:
        raise UnsupportedTexture(f"format {fmt}")
    m = width * height
    if m > 0x2000:
        # texdecompress.c:752 — the game gives up on the whole texture.
        raise UnsupportedTexture(f"{width}x{height} exceeds the 0x2000-texel scratch")
    if m == 0:
        raise UnsupportedTexture("zero-sized image")
    nch = FORMAT_NUM_CHANNELS[fmt]
    csize = FORMAT_CHANNEL_SIZES[fmt]

    def with_alpha(planes: list[int]) -> list[int]:
        s = planes[: nch * m] + [0] * (4 * m - nch * m)
        if FORMAT_HAS_1BIT_ALPHA[fmt]:
            s[3 * m : 4 * m] = [bs.read(1) for _ in range(m)]  # tex_read_alpha_bits, :1463
        return s

    if comp in (COMP_UNCOMPRESSED0, COMP_UNCOMPRESSED1):
        texels = _read_uncompressed(bs, width, height, fmt)
    elif comp == COMP_HUFFMAN:
        # One table across all channels (:762).
        texels = _channels_to_texels(with_alpha(_huffman(bs, nch * m, csize)), width, height, fmt)
    elif comp == COMP_HUFFMANPERCHANNEL:
        planes: list[int] = []
        for _ in range(nch):
            planes += _huffman(bs, m, csize)
        texels = _channels_to_texels(with_alpha(planes), width, height, fmt)
    elif comp == COMP_RLE:
        planes, _ = _rle(bs, nch * m)
        texels = _channels_to_texels(with_alpha(planes), width, height, fmt)
    elif comp == COMP_LOOKUP:
        lut = _build_lookup(bs, FORMAT_BITS_PER_PIXEL[fmt])
        bits = _bit_size(len(lut))
        if fmt in (FMT_IA4, FMT_I4):
            # :1773 — per row, the pair's second index only if it is in the row.
            idx = [0] * m
            for y in range(height):
                for x in range(0, width, 2):
                    idx[y * width + x] = bs.read(bits)
                    if x + 1 < width:
                        idx[y * width + x + 1] = bs.read(bits)
        else:
            idx = [bs.read(bits) for _ in range(m)]
        texels = _lookup_texels(idx, width, height, lut, fmt, from_buffer=False)
    elif comp in (COMP_HUFFMANLOOKUP, COMP_RLELOOKUP):
        lut = _build_lookup(bs, FORMAT_BITS_PER_PIXEL[fmt])
        if comp == COMP_HUFFMANLOOKUP:
            idx = _huffman(bs, m, len(lut))
            wide = len(lut) > 256  # :1311 stores u16 past 256 symbols
        else:
            idx, blocksize = _rle(bs, m)
            wide = blocksize > 8  # :1374 stores u16 past 8-bit blocks
        if wide != (len(lut) > 256):
            # :1811 picks u8/u16 from the table size alone; a mismatch would be
            # read back as byte-level garbage.
            raise UnsupportedTexture("lookup index width disagrees with table size")
        texels = _lookup_texels(idx, width, height, lut, fmt, from_buffer=True)
    elif comp in (COMP_HUFFMANBLUR, COMP_RLEBLUR):
        method = bs.read(3)  # read before the channel data (:805, :816)
        if comp == COMP_HUFFMANBLUR:
            planes = _huffman(bs, nch * m, csize)
        else:
            planes, _ = _rle(bs, nch * m)
            planes = planes[: nch * m]
        _blur(planes, width, nch * height, method, csize)
        texels = _channels_to_texels(with_alpha(planes), width, height, fmt)
    else:
        # The C switch has no default: `imagebytesout` is stale and nothing is read.
        raise UnsupportedTexture(f"compression method {comp}")
    return fmt, width, height, comp, texels


def _expand3(v: int) -> int:
    """3-bit -> 8-bit by bit replication (7 -> 255)."""
    return (v << 5) | (v << 2) | (v >> 1)


#: Per-format texel -> RGBA8, for the non-32-bit formats.
_TEXEL_CONVERT = {
    FMT_RGBA16: rgba5551,
    FMT_RGB15: rgba5551,
    FMT_IA16: ia16,
    FMT_IA8: lambda v: ((v >> 4) * 17,) * 3 + ((v & 15) * 17,),
    FMT_IA4: lambda v: (_expand3(v >> 1),) * 3 + (255 if v & 1 else 0,),
    FMT_I8: lambda v: (v, v, v, v),
    FMT_I4: lambda v: (v * 17,) * 4,
}


def texels_to_rgba(texels: list[int], fmt: int) -> bytes:
    """N64 texel values -> RGBA8.

    `I4`/`I8` put the intensity in alpha too — that is what the RDP does with an
    I texel. 4-bit channels scale by 17 (exact), IA4's 3-bit intensity by bit
    replication, 5-bit ones through [`rgba5551`] like the zlib path.
    """
    if fmt in (FMT_RGBA32, FMT_RGB24):
        return b"".join(v.to_bytes(4, "big") for v in texels)
    conv = _TEXEL_CONVERT.get(fmt)
    if conv is None:
        raise UnsupportedTexture(f"no RGBA conversion for {FORMAT_NAMES.get(fmt, fmt)}")
    cache: dict[int, bytes] = {}
    parts = []
    for v in texels:
        c = cache.get(v)
        if c is None:
            c = cache[v] = bytes(conv(v))
        parts.append(c)
    return b"".join(parts)


class NonZlibInfo:
    """Diagnostics from a walk of a non-zlib texture."""

    __slots__ = ("images", "consumed_bits", "total_bits", "notes")

    def __init__(self, images, consumed_bits, total_bits, notes=()):
        #: `(format, width, height, method)` per decoded image.
        self.images = images
        self.consumed_bits = consumed_bits
        self.total_bits = total_bits
        #: Non-fatal oddities, e.g. a truncated Huffman tree.
        self.notes = list(notes)


def inflate_non_zlib(payload: bytes, hasloddata: bool, numlods: int, all_images: bool = False):
    """`tex_inflate_non_zlib` (texdecompress.c:699) to level 0.

    Returns `((width, height, format, rgba), NonZlibInfo)`. With `all_images`,
    every *stored* LOD is decoded too (and discarded) — `numlods` should then be
    the unclamped header value, since six files store 7 — purely to check that
    the stream is consumed to its end: the strongest internal check available,
    since a wrong tree or field width desynchronises everything after it.

    Each image picks its own format and method (:736), so LOD 1+ may differ from
    LOD 0. `numimages` is `numlods` only when `hasloddata` (:720). No swizzle is
    undone: `tex_swizzle` (:829, :873, :886) runs after decode, for the RDP.
    """
    bs = BitString(payload)
    numimages = numlods if (hasloddata and numlods) else 1
    images = []
    first = None
    for i in range(numimages if all_images else 1):
        fmt, w, h, comp, texels = _inflate_image(bs)
        images.append((fmt, w, h, comp))
        if i == 0:
            first = (fmt, w, h, texels)
        bs.end_image()
    fmt, w, h, texels = first
    return (w, h, fmt, texels_to_rgba(texels, fmt)), NonZlibInfo(images, bs.pos, bs.limit, bs.notes)


def decode(data: bytes, _info: list | None = None, _all_images: bool = False) -> PoolTexture:
    """Decode a `textures/*.bin` to level 0 as tightly-packed RGBA8.

    Both codecs are handled. Raises [`UnsupportedTexture`] for an empty file
    (texture 0x0d42 has no data; `tex_load` returns early, texdecompress.c:2192)
    or a stream the game itself could not decode. `numcolours` is 0 for the
    non-paletted (non-zlib) textures. `_info`/`_all_images` are for `check`.
    """
    if len(data) < 2:
        raise UnsupportedTexture("empty texture")
    flags = data[0]
    hasloddata = bool(flags & 0x80)
    iszlib = bool(flags & 0x40)
    numlods = min(flags & 0x3F, 5)
    if not iszlib:
        walk = (flags & 0x3F) if _all_images else numlods
        (w, h, fmt, rgba), info = inflate_non_zlib(data[1:], hasloddata, walk, _all_images)
        if _info is not None:
            _info.append(info)
        return PoolTexture(w, h, fmt, rgba, numlods, hasloddata, 0)

    payload = data[1:]
    br = BitReader(payload)
    fmt = br.read(8)
    numcolours = br.read(8) + 1
    palette = [br.read(16) for _ in range(numcolours)]

    ipb = INDICES_PER_BYTE.get(fmt)
    if ipb is None:
        # Every zlib-path texture in the shipped set is paletted; a non-CI one
        # here would mean `tex_align_indices` running on uninitialised state.
        raise UnsupportedTexture(f"zlib path with non-paletted format {FORMAT_NAMES.get(fmt, fmt)}")

    # Only level 0 is wanted; the engine regenerates the rest.
    off = br.byte_pos
    width, height = payload[off], payload[off + 1]
    indices, _ = rzip_inflate(payload, off + 2)

    to_rgba = ia16 if fmt in IA_PALETTE else rgba5551
    lut = [to_rgba(v) for v in palette]
    stride = (width + ipb - 1) // ipb  # packed rows; padding is applied later by
    #                                     `tex_align_indices`, not here
    need = stride * height
    if len(indices) < need:
        raise UnsupportedTexture(f"inflated {len(indices)} bytes, need {need} for {width}x{height}")

    px = bytearray(width * height * 4)
    for y in range(height):
        row = y * stride
        for x in range(width):
            if ipb == 2:
                b = indices[row + (x >> 1)]
                idx = (b >> 4) if (x & 1) == 0 else (b & 0xF)
            else:
                idx = indices[row + x]
            r, g, bl, a = lut[idx] if idx < len(lut) else (255, 0, 255, 255)
            d = (y * width + x) * 4
            px[d] = r
            px[d + 1] = g
            px[d + 2] = bl
            px[d + 3] = a
    return PoolTexture(width, height, fmt, bytes(px), numlods, hasloddata, numcolours)


def assets_root() -> str:
    return os.path.join(
        os.path.dirname(os.path.abspath(__file__)),
        "..", "..", "reference", "pd-decomp", "src", "assets", "ntsc-final",
    )


def load(texturenum: int) -> PoolTexture:
    """Decode pool texture `texturenum` (as `textureconfig.texturenum` gives it)."""
    path = os.path.join(assets_root(), "textures", f"{texturenum:04x}.bin")
    with open(path, "rb") as fh:
        return decode(fh.read())


# ---------------------------------------------------------------------------


def cmd_info(paths) -> int:
    ok = bad = 0
    for p in paths:
        with open(p, "rb") as fh:
            data = fh.read()
        try:
            info: list = []
            t = decode(data, info, _all_images=True)
            ok += 1
            if info:
                how = "non-zlib " + ",".join(COMP_NAMES.get(c, str(c)) for _f, _w, _h, c in info[0].images)
                if info[0].notes:
                    how += f"  [{'; '.join(info[0].notes)}]"
            else:
                how = f"zlib, {t.numcolours} colours"
            print(
                f"{os.path.basename(p):<12} {t.width:3}x{t.height:<3} {t.format_name:<11} "
                f"lods={t.numlods} hasloddata={int(t.hasloddata)}  {how}"
            )
        except UnsupportedTexture as e:
            bad += 1
            print(f"{os.path.basename(p):<12} UNSUPPORTED: {e}")
    print(f"\n{ok} decoded, {bad} unsupported")
    return 0


def cmd_check(verbose: bool = False) -> int:
    """Decode every pool texture — all stored LODs, not just level 0 — and
    summarise by codec, format and method.

    Besides "did it raise", every non-zlib texture is held to a consumption
    check: after the last image (and the inter-image step) the bit cursor must
    land exactly on the end of the file. Huffman and RLE streams have no length
    field, so a mis-built tree or a wrong field width cannot land there by luck.
    """
    folder = os.path.join(assets_root(), "textures")
    names = sorted(f for f in os.listdir(folder) if f.endswith(".bin"))
    ok: dict[str, int] = {}
    methods: dict[str, int] = {}
    failed: list[tuple[str, str]] = []
    short: list[tuple[str, int, str]] = []
    for name in names:
        with open(os.path.join(folder, name), "rb") as fh:
            data = fh.read()
        info: list = []
        try:
            t = decode(data, info, _all_images=True)
        except UnsupportedTexture as e:
            failed.append((name, str(e)))
            continue
        codec = "zlib" if not info else "non-zlib"
        key = f"{codec:<8} {t.format_name}"
        ok[key] = ok.get(key, 0) + 1
        if info:
            for fmt, _w, _h, comp in info[0].images:
                mk = f"{FORMAT_NAMES.get(fmt, fmt)}/{COMP_NAMES.get(comp, comp)}"
                methods[mk] = methods.get(mk, 0) + 1
            left = info[0].total_bits - info[0].consumed_bits
            if left != 0:
                short.append((name, left, "; ".join(info[0].notes)))
    print(f"{len(names)} files, {sum(ok.values())} decoded, {len(failed)} failed\n")
    print("decoded, by codec and level-0 format:")
    for k in sorted(ok):
        print(f"  {k:<24} {ok[k]:5}")
    print("\nnon-zlib images (every LOD), by format/method:")
    for k in sorted(methods):
        print(f"  {k:<28} {methods[k]:5}")
    unexplained = [x for x in short if not x[2]]
    print(f"\nnon-zlib streams not ending exactly at end of file: {len(short)} "
          f"({len(short) - len(unexplained)} explained by a game-side truncated Huffman tree)")
    for name, left, why in short[: (None if verbose else 20)]:
        print(f"  {name}  {left:+d} bits left over" + (f"  ({why})" if why else ""))
    if failed:
        print("\nfailed:")
        for name, why in failed:
            print(f"  {name}  {why}")
    return 0 if not unexplained and all(why == "empty texture" for _, why in failed) else 1


def texture_configs() -> dict[int, list[tuple[int, int, str, str]]]:
    """`texturenum -> [(width, height, G_IM_FMT, G_IM_SIZ)]` from the global
    `src/textureconfig.c` tables — the size and format the game *draws* each
    effect texture at, which `png` cross-checks the decode against."""
    path = os.path.join(assets_root(), "..", "..", "textureconfig.c")
    out: dict[int, list] = {}
    if not os.path.exists(path):
        return out
    rx = re.compile(r"\{\s*TEXTURE_([0-9A-Fa-f]{4})\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*\d+\s*,\s*G_IM_FMT_(\w+)\s*,\s*G_IM_SIZ_(\w+)")
    with open(path, encoding="utf-8", errors="replace") as fh:
        for m in rx.finditer(fh.read()):
            out.setdefault(int(m.group(1), 16), []).append(
                (int(m.group(2)), int(m.group(3)), m.group(4), m.group(5)))
    return out


#: TEXFORMAT_* -> the GBI (format, size) it is drawn as (texdecompress.c:66-96).
GBI_OF_FORMAT = {
    FMT_RGBA32: ("RGBA", "32b"), FMT_RGBA16: ("RGBA", "16b"), FMT_RGB24: ("RGBA", "32b"),
    FMT_RGB15: ("RGBA", "16b"), FMT_IA16: ("IA", "16b"), FMT_IA8: ("IA", "8b"),
    FMT_IA4: ("IA", "4b"), FMT_I8: ("I", "8b"), FMT_I4: ("I", "4b"),
    FMT_RGBA16_CI8: ("CI", "8b"), FMT_RGBA16_CI4: ("CI", "4b"),
    FMT_IA16_CI8: ("CI", "8b"), FMT_IA16_CI4: ("CI", "4b"),
}


def cmd_png(outdir: str, nums: list[int], manifest: str | None) -> int:
    """Write level 0 of pool textures as `tex_NNNN.png` (RGBA), plus an optional
    JSON manifest `{"0xNNNN": {file, w, h, format, source, codec}}`.

    Each texture's size is cross-checked against its `textureconfig.c` entries,
    independent evidence the header was read right. Their *format* fields are
    only reported: the game draws with the decoded `tex->gbiformat`/`depth`
    (`tex.c:438`, `:498`), and six of them are stale (0x0854-0x0856 and 0x08f0
    say IA16 but are IA8; 0x0c97 says IA8 but is RGBA32; 0x0007 says RGBA32 but
    is CI8).
    """
    import json

    from pd_gltf import png_bytes  # noqa: E402

    os.makedirs(outdir, exist_ok=True)
    configs = texture_configs()
    entries = {}
    bad = 0
    for n in nums:
        path = os.path.join(assets_root(), "textures", f"{n:04x}.bin")
        with open(path, "rb") as fh:
            data = fh.read()
        t = decode(data)
        fname = f"tex_{n:04x}.png"
        with open(os.path.join(outdir, fname), "wb") as fh:
            fh.write(png_bytes(t.width, t.height, t.rgba))
        codec = "zlib" if data[0] & 0x40 else "non-zlib"
        entries[f"0x{n:04x}"] = {
            "file": fname, "w": t.width, "h": t.height, "format": t.format_name,
            "source": "pd", "codec": codec,
        }
        alpha = t.rgba[3::4]
        rgb = [max(t.rgba[i : i + 3]) for i in range(0, len(t.rgba), 4)]
        opaque = sum(1 for a in alpha if a == 255)
        clear = sum(1 for a in alpha if a == 0)
        mismatch, fmtnote = [], []
        for cw, ch, cf, cs in configs.get(n, []):
            if (cw, ch) != (t.width, t.height):
                mismatch.append(f"texconfig says {cw}x{ch}")
            if (cf, cs) != GBI_OF_FORMAT[t.format]:
                fmtnote.append(f"texconfig fmt {cf} {cs} (unused)")
        bad += bool(mismatch)
        print(
            f"{fname}  {t.width:3}x{t.height:<3} {t.format_name:<7} {codec:<8} "
            f"alpha min/mean/max {min(alpha):3}/{sum(alpha) / len(alpha):6.1f}/{max(alpha):3} "
            f"(opaque {opaque * 100 // len(alpha):3}%, clear {clear * 100 // len(alpha):3}%)  "
            f"rgb max {max(rgb):3}  distinct {len(set(t.rgba[i:i + 4] for i in range(0, len(t.rgba), 4))):4}"
            + (f"  !! {'; '.join(sorted(set(mismatch)))}" if mismatch else
               ("  texconfig size ok" if n in configs else "  (no texconfig)"))
            + (f"; {'; '.join(sorted(set(fmtnote)))}" if fmtnote else "")
        )
    if manifest:
        with open(os.path.join(outdir, manifest), "w", encoding="utf-8", newline="\n") as fh:
            json.dump(entries, fh, indent=1)
            fh.write("\n")
    print(f"\n{len(nums)} textures -> {outdir}" + (f" (+ {manifest})" if manifest else "")
          + (f"; {bad} disagree with textureconfig.c on size" if bad else ""))
    return 0


def cmd_sheet(out: str, paths, cell: int = 80) -> int:
    from pd_gltf import png_bytes  # noqa: E402

    tiles = []
    for p in paths:
        with open(p, "rb") as fh:
            data = fh.read()
        try:
            tiles.append((os.path.basename(p), decode(data)))
        except UnsupportedTexture:
            continue
    if not tiles:
        raise SystemExit("nothing decoded")
    cols = min(8, len(tiles))
    rows = (len(tiles) + cols - 1) // cols
    tw, th = cols * cell, rows * cell
    sheet = bytearray()
    for _ in range(tw * th):
        sheet += bytes((30, 32, 36, 255))
    for i, (_name, t) in enumerate(tiles):
        cx, cy = (i % cols) * cell, (i // cols) * cell
        sc = min((cell - 8) / t.width, (cell - 8) / t.height)
        dw, dh = max(int(t.width * sc), 1), max(int(t.height * sc), 1)
        ox, oy = cx + (cell - dw) // 2, cy + (cell - dh) // 2
        for y in range(dh):
            for x in range(dw):
                s = (int(y / sc) * t.width + int(x / sc)) * 4
                d = ((oy + y) * tw + (ox + x)) * 4
                if t.rgba[s + 3] == 0:
                    v = 95 if ((x // 6 + y // 6) % 2) else 60
                    sheet[d : d + 4] = bytes((v, v, v, 255))
                else:
                    sheet[d : d + 3] = t.rgba[s : s + 3]
    with open(out, "wb") as fh:
        fh.write(png_bytes(tw, th, bytes(sheet)))
    print(f"{len(tiles)} textures -> {out}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("info", help="decode and summarise")
    p.add_argument("textures", nargs="+")
    p = sub.add_parser("sheet", help="render a contact sheet")
    p.add_argument("out")
    p.add_argument("textures", nargs="+")
    p.add_argument("--cell", type=int, default=80)
    p = sub.add_parser("check", help="decode every pool texture and summarise")
    p.add_argument("-v", "--verbose", action="store_true")
    p = sub.add_parser("png", help="export pool textures (by number) as tex_NNNN.png")
    p.add_argument("outdir")
    p.add_argument("nums", nargs="+", help="texture numbers, hex (0x0003 or 0003)")
    p.add_argument("--manifest", help="also write this JSON file into outdir")
    args = ap.parse_args()
    if args.cmd == "info":
        return cmd_info(args.textures)
    if args.cmd == "check":
        return cmd_check(args.verbose)
    if args.cmd == "png":
        return cmd_png(args.outdir, [int(x, 16) for x in args.nums], args.manifest)
    return cmd_sheet(args.out, args.textures, args.cell)


if __name__ == "__main__":
    sys.exit(main())
