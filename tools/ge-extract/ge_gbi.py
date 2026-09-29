"""What GoldenEye's display lists need of PD's interpreter (`pd_fpgun.Interp`).

GE's DLs are PD's dialect (fast3d, Rare's G_TRI4 0xB1, the C0 texture
command with the same fields: `tex.c:813-975` against PD's `tex.c:823`). Three
things differ, and this mixin, put before an `Interp` class, supplies them:

* **The vertex**: fast3d's own 16-byte `Vtx_t`, the colour inline
  (`include/PR/gbi.h:1112`), not PD's 12-byte one indexing a G_COL table.
* **The images**: a C0 command names a GE image, whose flags byte says whether
  it stores its own mip images (the half-texel offset, `tex.c`).
* **The blender's FORCE_BL**: GE's opaque rooms draw with `G_RM_PASS` then
  `(CLR_IN, A_IN, CLR_MEM, 1MA)` with antialiasing, `ALPHA_CVG_SEL` and no
  `FORCE_BL` (`0x0C182078`; PD's rooms use `..., A_MEM`). The RDP blends
  without `FORCE_BL` only where a pixel is partly covered, the polygon's
  antialiased edge, and writes a covered pixel's colour as it is: the surface
  is opaque. `pd_fpgun` follows fast3d there (blend iff `M = CLR_MEM, B =
  1MA`), which no PD asset exercises; for GE the RDP's rule is applied.
"""

from __future__ import annotations

import sys

from ge_rom import PD_ASSETS_TOOLS

sys.path.insert(0, PD_ASSETS_TOOLS)
import pd_fpgun  # noqa: E402


class GeDialect:
    vtx_size = 16

    def hasloddata(self, texnum: int) -> bool | None:
        return self.images.hasloddata(texnum)

    def blend_state(self, oml: int, two_cycle: bool) -> dict:
        d = pd_fpgun.blend_from_othermode(oml, two_cycle)
        if not oml & pd_fpgun.FORCE_BL:
            d["blend"] = "opaque"
        return d
