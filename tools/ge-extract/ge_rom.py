"""GoldenEye 007 (NTSC, `ge007.u`): the ROM's files and the decomp's tables.

The ROM is the data: every BG, clipping ("stan"), model and image is read from
it at the offsets the decomp's own extractor uses (`scripts/filelist.u.csv`,
`imagelist.u.csv`: `offset,size,path,compressed,extract`). The decomp
(`reference/ge-decomp`, n64decomp/007) supplies what the game's code knows: the
level table (`bg.c:184`), the image table (`assets/images.def`), the prop
model table and model headers (`assets/obseg/prop/`), and the setups, which it
holds as generated C (`assets/obseg/setup/`).

A v64 (byte-swapped) or n64 (word-swapped) dump is put in z64 order first; the
result must be the ROM the decomp builds (`ge007.u.sha1`), or the offsets are
someone else's.
"""

from __future__ import annotations

import csv
import hashlib
import os
import re
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
PD_ASSETS_TOOLS = os.path.join(REPO, "tools", "pd-assets")


class GeError(Exception):
    pass


def inflate1172(data: bytes, off: int = 0) -> bytes:
    """GE's rzip: `0x11 0x72`, then raw DEFLATE to its end block, no length
    (`decompress.c:53-66`). PD's is `0x11 0x73` + a u24 length."""
    if data[off : off + 2] != b"\x11\x72":
        raise GeError(f"expected a 1172 stream at {off:#x}, found {data[off:off + 2].hex()}")
    d = zlib.decompressobj(-15)
    out = d.decompress(data[off + 2 :])
    if not d.eof:
        raise GeError(f"1172 stream at {off:#x} is truncated")
    return out


def z64(raw: bytes) -> bytes:
    """A dump in the ROM's own (big-endian) byte order."""
    b = bytearray(raw)
    head = bytes(b[:4])
    if head == b"\x80\x37\x12\x40":
        return bytes(b)
    if head == b"\x37\x80\x40\x12":  # v64: every 16-bit pair swapped
        b[0::2], b[1::2] = b[1::2], b[0::2]
        return bytes(b)
    if head == b"\x40\x12\x37\x80":  # n64: every 32-bit word reversed
        for i in range(0, len(b) - 3, 4):
            b[i : i + 4] = b[i : i + 4][::-1]
        return bytes(b)
    raise GeError(f"not an N64 ROM (starts {head.hex()})")


class Ge:
    """The ROM and the decomp, read once."""

    def __init__(self, rom_path: str, decomp: str) -> None:
        self.decomp = decomp
        with open(rom_path, "rb") as fh:
            self.rom = z64(fh.read())
        want = self._read("ge007.u.sha1").split()[0].lower()
        got = hashlib.sha1(self.rom).hexdigest()
        if got != want:
            raise GeError(f"{rom_path}: sha1 {got} is not ge007.u's {want} (the NTSC ROM the decomp builds)")
        self.files: dict[str, tuple[int, int, bool]] = {}
        with open(os.path.join(decomp, "scripts", "filelist.u.csv"), newline="") as fh:
            for row in csv.reader(fh):
                if len(row) >= 4:
                    stem = os.path.splitext(os.path.basename(row[2]))[0]
                    self.files[stem] = (int(row[0]), int(row[1]), row[3].strip() == "1")
        with open(os.path.join(decomp, "imagelist.u.csv"), newline="") as fh:
            self.images = [(int(r[0]), int(r[1])) for r in csv.reader(fh) if r]

    def _read(self, *rel: str) -> str:
        with open(os.path.join(self.decomp, *rel), encoding="utf-8", errors="replace") as fh:
            return fh.read()

    def rel(self, path: str) -> str:
        """`path` as provenance: relative to the decomp's parent (`ge-decomp/...`)."""
        return os.path.relpath(path, os.path.dirname(self.decomp)).replace(os.sep, "/")

    # -- files ----------------------------------------------------------------

    def file(self, stem: str) -> bytes:
        """ROM file `stem` (e.g. `bg_ark_all_p`, `Tbg_ark_all_p_stanZ`,
        `Pgas_plant_sw2_do1Z`), inflated if the table says it is compressed."""
        if stem not in self.files:
            raise GeError(f"{stem} is not in scripts/filelist.u.csv")
        off, size, compressed = self.files[stem]
        data = self.rom[off : off + size]
        return inflate1172(data) if compressed else data

    def image(self, n: int) -> bytes:
        """Image `n` of the global bank, as stored (`texLoad`, `image.c:2371`)."""
        off, size = self.images[n]
        return self.rom[off : off + size]

    # -- the decomp's tables --------------------------------------------------

    def level(self, levelid: str) -> dict:
        """`levelinfotable`'s row (`bg.c:184`): the BG and stan files and the
        level scale (BG units per world unit, `bgroomtrans.c:211`)."""
        text = self._read("src", "game", "bg.c")
        m = re.search(rf"\{{\s*{levelid}\s*,\s*\"bg/(\w+)\.seg\"\s*,\s*\"(\w+)\"\s*,\s*([-0-9.e]+)\s*,\s*([-0-9.e]+)\s*,\s*([-0-9.e]+)\s*\}}", text)
        if not m:
            raise GeError(f"{levelid} is not in bg.c's levelinfotable")
        return {"bg": m.group(1), "stan": m.group(2), "levelscale": float(m.group(3)), "visibility": float(m.group(4)),
                "source": "ge-decomp/src/game/bg.c (levelinfotable)"}

    def image_hits(self) -> list[tuple[int, int]]:
        """Each image's two hit types (`assets/images.def`: the sound one, then
        the effect one). GE's `HIT_*` (`bondconstants.h:1580`) is PD's
        `SURFACETYPE_*` in the same order (0 default .. 12 glass xlu)."""
        hits = ["HIT_DEFAULT", "HIT_STONE", "HIT_WOOD", "HIT_METAL", "HIT_GLASS", "HIT_WATER", "HIT_SNOW", "HIT_DIRT",
                "HIT_MUD", "HIT_TILE", "HIT_METALOBJ", "HIT_CHR", "HIT_GLASS_XLU"]
        out = []
        for m in re.finditer(r"IMAGE\(\s*\w+\s*,\s*\w+\s*,\s*(\w+)\s*,\s*(\w+)", self._read("assets", "images.def")):
            out.append((hits.index(m.group(1)), hits.index(m.group(2))))
        return out

    def props(self) -> list[str]:
        """`PitemZ_entries[]` (`loadobjectmodel.c:335`): the model directory of
        each prop number (`enum PROP`, the setup's `obj`)."""
        inc = self._read("assets", "obseg", "prop", "propItemModelFileRecord.inc.c")
        return re.findall(r"assets/obseg/prop/([^/]+)/propFileRecord", inc)

    def prop_record(self, name: str) -> tuple[str, float]:
        """`PROPFILERECORD(name, scale)`: the file `P<name>Z` and the model's
        scale (0.1 when empty, `bondconstants.h:4824`)."""
        rec = self._read("assets", "obseg", "prop", name, "propFileRecord.inc.c")
        m = re.search(r"PROPFILERECORD\(\s*(\w+)\s*,\s*([^)]*)\)", rec)
        if not m:
            raise GeError(f"no PROPFILERECORD in prop/{name}")
        return f"P{m.group(1)}Z", float(m.group(2).strip() or 0.1)

    def model_header(self, name: str) -> dict:
        """`MODELFILEHEADER(name, root, skeleton, switches, numswitches,
        nummatrices, radius, numrecords, numtextures)` (`bondconstants.h:4808`):
        GE keeps a model's header in the game, not in its file."""
        text = self._read("assets", "obseg", "prop", name, "ModelFileHeader.inc.c")
        m = re.search(r"MODELFILEHEADER\((.*)\)", text)
        if not m:
            raise GeError(f"no MODELFILEHEADER in prop/{name}")
        args = [a.strip() for a in re.split(r",(?![^(]*\))", m.group(1))]
        skel = re.search(r"SKELETON\((\w+)\)", args[2])
        return {"skeleton": skel.group(1) if skel else None, "numswitches": int(args[4], 0), "nummatrices": int(args[5], 0),
                "radius": float(args[6]), "numtextures": int(args[8], 0)}

    def setup_c(self, name: str) -> tuple[str, str]:
        """The setup `name` (e.g. `UsetuparkZ`) as the decomp holds it, and its path."""
        path = os.path.join(self.decomp, "assets", "obseg", "setup", name + ".c")
        with open(path, encoding="utf-8", errors="replace") as fh:
            return fh.read(), path
