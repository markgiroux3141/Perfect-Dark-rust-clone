"""GoldenEye's global image bank into PNGs, by PD's decoder.

GE's image format is PD's ancestor (`image.c:2371` `texLoad` against PD's
`texdecompress.c`): the same flags byte, the same zlib (paletted) and non-zlib
(Huffman, RLE, lookup, blur) paths, the same thirteen formats. The one
difference, GE's `0x11 0x72` rzip header, `pd_tex.rzip_inflate` accepts, so
`pd_tex.decode` is the decoder (all 2698 images decode; the GE Setup Editor's
Facility BMPs match it pixel for pixel, allowing for the editor's row order and
shift expansion).

A level's images are written once to `<custom>/ge/tex/<nnnn>.png` (four hex
digits, the image number), shared by every GE level and model that draws them;
the texture entries point there (`AssetDir::path` finds them in the custom tree).
"""

from __future__ import annotations

import os
import sys

from ge_rom import PD_ASSETS_TOOLS, Ge

sys.path.insert(0, PD_ASSETS_TOOLS)
import pd_gltf  # noqa: E402
import pd_tex  # noqa: E402

#: The id a GE image takes in a model or BG file: past PD's pool (`pd_core::model`:
#: `0x10000 |` a texture the file carries rather than the pool's).
def texture_id(n: int) -> int:
    return 0x10000 | n


class Images:
    """Decodes GE images on demand and writes each used one's PNG once."""

    def __init__(self, ge: Ge, custom: str) -> None:
        self.ge = ge
        self.custom = custom
        self.hits = ge.image_hits()
        self.decoded: dict[int, pd_tex.PoolTexture] = {}
        self.written: set[int] = set()

    def get(self, n: int) -> pd_tex.PoolTexture:
        t = self.decoded.get(n)
        if t is None:
            t = self.decoded[n] = pd_tex.decode(self.ge.image(n))
        return t

    def hasloddata(self, n: int) -> bool:
        return bool(self.ge.image(n)[0] & 0x80)

    def config(self, n: int) -> "pd_gltf.TexConfig":
        """A `read_texconfigs`-shaped entry: the texture is its own config, as
        on a PD BG (`pd_bg.texture_configs`); GE has no texture config table for
        world geometry (`texLoadFromGdl` takes the size from the image)."""
        t = self.get(n)
        return pd_gltf.TexConfig(n, n, t.width, t.height, t.numlods, 0, 0, texnum=texture_id(n))

    def entry(self, n: int) -> dict:
        """Write image `n`'s PNG (once) and return its texture entry."""
        t = self.get(n)
        rel = f"ge/tex/{n:04x}.png"
        if n not in self.written:
            path = os.path.join(self.custom, *rel.split("/"))
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "wb") as fh:
                fh.write(pd_gltf.png_bytes(t.width, t.height, t.rgba))
            self.written.add(n)
        sound, surface = self.hits[n]
        return {"file": rel, "w": t.width, "h": t.height, "cfg_w": t.width, "cfg_h": t.height, "levels": t.numlods,
                "source": "ge", "surfacetype": surface, "soundsurfacetype": sound}
