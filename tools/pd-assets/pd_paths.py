"""Where the exporters read from and write to. Every exporter imports this; none
builds a decomp or output path of its own.

* **The decomp** (the input): `PD_DECOMP_DIR` if set, else `reference/pd-decomp`.
  Its `tools/extract` must have run once against the ROM (reference/README.md).
* **The asset root** (the output): `PD_ASSETS_OUT` if set, else `assets/` at the
  repo root. The layout under it is docs/ARCHITECTURE.md § Assets.

Provenance strings name decomp files relative to the decomp root
(`pd-decomp/src/...`) so they read the same on every machine.
"""

from __future__ import annotations

import os

HERE = os.path.dirname(os.path.abspath(__file__))
#: The repository root (two levels above tools/pd-assets).
REPO = os.path.dirname(os.path.dirname(HERE))

DECOMP = os.path.abspath(os.environ.get("PD_DECOMP_DIR") or os.path.join(REPO, "reference", "pd-decomp"))
#: The decomp's C sources and headers.
SRC = os.path.join(DECOMP, "src")
#: What the decomp's `tools/extract` unpacked from the NTSC-final ROM (plus the JSON it commits).
ASSETS = os.path.join(SRC, "assets", "ntsc-final")
#: Raw ROM segments `tools/extract` leaves outside `src/` (mpconfigs, mpstrings).
EXTRACTED = os.path.join(DECOMP, "extracted", "ntsc-final")

OUT = os.path.abspath(os.environ.get("PD_ASSETS_OUT") or os.path.join(REPO, "assets"))


def src(*parts: str) -> str:
    """A path under the decomp's `src/`."""
    return os.path.join(SRC, *parts)


def asset(*parts: str) -> str:
    """A path under the decomp's extracted `src/assets/ntsc-final/`."""
    return os.path.join(ASSETS, *parts)


def out(*parts: str) -> str:
    """A path under the output asset root."""
    return os.path.join(OUT, *parts)


def decomp_rel(path: str) -> str:
    """`path` as provenance: `pd-decomp/<path relative to the decomp root>`."""
    return "pd-decomp/" + os.path.relpath(os.path.abspath(path), DECOMP).replace(os.sep, "/")


def out_rel(path: str) -> str:
    """`path` relative to the asset root, with forward slashes (what manifests store)."""
    return os.path.relpath(os.path.abspath(path), OUT).replace(os.sep, "/")


def require_decomp() -> None:
    """Fail early, with the fix, if the decomp or its extraction is missing."""
    if not os.path.isdir(SRC):
        raise SystemExit(f"no decomp at {DECOMP} (set PD_DECOMP_DIR, see reference/README.md)")
    if not os.path.isdir(os.path.join(ASSETS, "files")):
        raise SystemExit(f"{DECOMP} is not extracted: run `ROMID=ntsc-final python tools/extract` there "
                         "(reference/README.md)")
