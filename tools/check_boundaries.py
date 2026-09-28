"""Fail if a headless crate links a GPU, window, audio or UI crate.

    python tools/check_boundaries.py

The rule is in docs/ARCHITECTURE.md: n64 (without its `gpu` feature), pd_core,
pd_sim and pd_menu never depend on wgpu, winit, kira, gilrs or egui, directly or
transitively; nothing below `engine` depends on `engine`; and the engine's
source never names the game or the console.
"""
import os
import re
import subprocess
import sys

HEADLESS = ["n64", "pd_core", "pd_sim", "pd_menu"]
FORBIDDEN = {"wgpu", "winit", "kira", "gilrs", "egui", "egui-wgpu", "egui-winit", "engine"}


def deps(crate: str) -> set[str]:
    out = subprocess.run(
        ["cargo", "tree", "-p", crate, "-e", "normal", "--prefix", "none"],
        check=True, capture_output=True, text=True,
    ).stdout
    return {line.split(" ", 1)[0] for line in out.splitlines() if line.strip()}


#: The engine knows nothing about the game or the console, not even in comments.
ENGINE_WORDS = re.compile(r"\bPD\b|\bN64\b|\bn64\b|Perfect Dark|\bpd_[a-z]+")


def engine_vocabulary() -> list[str]:
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "engine")
    hits = []
    for dirpath, _, files in os.walk(root):
        for name in files:
            if not name.endswith((".rs", ".toml", ".wgsl")):
                continue
            path = os.path.join(dirpath, name)
            with open(path, encoding="utf-8") as f:
                for n, line in enumerate(f, 1):
                    if ENGINE_WORDS.search(line):
                        hits.append(f"{os.path.relpath(path, root)}:{n}: {line.strip()}")
    return hits


def main() -> int:
    bad = 0
    for crate in HEADLESS:
        hits = sorted(deps(crate) & FORBIDDEN)
        print(f"{crate:8} {'ok' if not hits else 'FORBIDDEN: ' + ', '.join(hits)}")
        bad += bool(hits)
    words = engine_vocabulary()
    print(f"{'engine':8} {'ok' if not words else 'mentions the game:'}")
    for w in words:
        print(f"         {w}")
    bad += bool(words)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
