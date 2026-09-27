"""Fail if a headless crate links a GPU, window, audio or UI crate.

    python tools/check_boundaries.py

The rule is in docs/ARCHITECTURE.md: n64 (without its `gpu` feature), pd_core,
pd_sim and pd_menu never depend on wgpu, winit, kira, gilrs or egui, directly or
transitively, and nothing below `engine` depends on `engine`.
"""
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


def main() -> int:
    bad = 0
    for crate in HEADLESS:
        hits = sorted(deps(crate) & FORBIDDEN)
        print(f"{crate:8} {'ok' if not hits else 'FORBIDDEN: ' + ', '.join(hits)}")
        bad += bool(hits)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
