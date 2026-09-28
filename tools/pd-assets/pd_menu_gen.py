#!/usr/bin/env python3
"""Generate the `pd_menu` spike's data from the Perfect Dark decomp, verbatim.

The Combat Simulator menus are data: ~70 `struct menudialogdef`s and their
`struct menuitem[]` arrays (src/game/mplayer/setup.c, mplayer/scenarios.c + the
scenario .inc files, and the "Perfect Menu" from src/game/mainmenu.c), plus the
MP tables they read (g_MpArenas, g_MpBodies, g_MpHeads, g_BotProfiles,
g_MpWeapons, g_MpWeaponSets, g_MpTracks, g_MpPresets, g_MpChallenges,
g_MpScenarioOverviews, g_HeadsAndBodies, the weapon names from invitems.c).
Rather than hand-copy ~5000 lines of initialisers, this script parses them out
of the decomp (NTSC-final: VERSION = VERSION_NTSC_FINAL, PAL = 0) and writes
Rust statics with the same shape, so every flag, text id and parameter is PD's.

`export_assets` writes the runtime assets under the asset root (pd_paths.OUT):
fonts/ (Handel Gothic xs/sm/md/lg, numeric), lang/en.json (every bank, keyed by
LANGBANK_*), lang/mpstringsE.bin and data/mpconfigs.bin (presets and challenges
live there as data). build_assets.py calls it. The menu textures are in the
texture pool (pd_models.py).

Usage:
    python tools/pd-assets/pd_menu_gen.py            # writes both; build_assets.py
                                                     # calls write_rust + export_assets
"""

from __future__ import annotations

import json
import os
import re
import shutil
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from pd_paths import DECOMP, EXTRACTED, REPO as ROOT, SRC, out  # noqa: E402

# The menu tables, generated into the crate that owns them (M2 migrates pd_menu).
OUT_RS = os.path.join(ROOT, "crates", "pd_menu", "src", "generated.rs")
#: `g_MpWeapons` as Rust, in pd_core (the match reads it too).
MP_WEAPONS_RS = os.path.join(ROOT, "crates", "pd_core", "src", "mpweapons.rs")
MP_WEAPONS_ROWS: list[str] = []
#: fonts/, lang/ and data/ go straight under the asset root.
OUT_ASSETS = out()

VERSION = 2  # VERSION_NTSC_FINAL
VERSIONS = {
    "VERSION_NTSC_BETA": 0,
    "VERSION_NTSC_1_0": 1,
    "VERSION_NTSC_FINAL": 2,
    "VERSION_PAL_BETA": 3,
    "VERSION_PAL_FINAL": 4,
    "VERSION_JPN_FINAL": 5,
    "VERSION": VERSION,
    "PAL": 0,
    "PIRACYCHECKS": 0,
    "MATCHING": 1,
}


# --------------------------------------------------------------------------
# Preprocessing
# --------------------------------------------------------------------------

def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    text = re.sub(r"//[^\n]*", "", text)
    return text


def c_expr_to_py(expr: str) -> str:
    expr = re.sub(r"defined\s*\(\s*(\w+)\s*\)", lambda m: "1" if m.group(1) in VERSIONS else "0", expr)
    expr = expr.replace("&&", " and ").replace("||", " or ")
    expr = re.sub(r"!(?!=)", " not ", expr)
    return expr


def eval_cond(expr: str, env: dict) -> bool:
    py = c_expr_to_py(expr)

    def name(m):
        n = m.group(0)
        if n in ("and", "or", "not"):
            return n
        if n in env:
            return str(env[n])
        return "0"

    py = re.sub(r"[A-Za-z_]\w*", name, py)
    return bool(eval(py))


def preprocess(text: str, env: dict | None = None) -> str:
    """Resolve #if/#ifdef/#elif/#else/#endif for NTSC-final; drop other directives."""
    env = dict(VERSIONS) if env is None else env
    out = []
    stack = []  # (active_now, taken_any, parent_active)
    active = True
    for line in text.split("\n"):
        s = line.strip()
        m = re.match(r"#\s*(if|ifdef|ifndef|elif|else|endif)\b(.*)", s)
        if m:
            d, rest = m.group(1), m.group(2).strip()
            if d in ("if", "ifdef", "ifndef"):
                if d == "if":
                    c = eval_cond(rest, env) if active else False
                elif d == "ifdef":
                    c = rest in env
                else:
                    c = rest not in env
                stack.append((c, c, active))
                active = active and c
            elif d == "elif":
                cur, taken, parent = stack.pop()
                c = (not taken) and parent and eval_cond(rest, env)
                stack.append((c, taken or c, parent))
                active = parent and c
            elif d == "else":
                cur, taken, parent = stack.pop()
                c = not taken
                stack.append((c, True, parent))
                active = parent and c
            else:
                _, _, parent = stack.pop()
                active = parent
            out.append("")
            continue
        if s.startswith("#"):
            out.append("")
            continue
        out.append(line if active else "")
    return "\n".join(out)


def read(path: str) -> str:
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


# --------------------------------------------------------------------------
# Constants: #defines and enums from the headers
# --------------------------------------------------------------------------

class Consts:
    def __init__(self):
        self.defs: dict[str, str] = {}
        self.vals: dict[str, int] = {}

    def load_header(self, path: str):
        text = preprocess(strip_comments(read(path)).replace("\\\n", " "))
        # #defines (object-like) — read from the raw text with conditionals applied
        raw = preprocess_keep_defines(read(path))
        for m in re.finditer(r"^[ \t]*#define[ \t]+([A-Za-z_]\w*)[ \t]+([^\n]*)$", raw, re.M):
            name, val = m.group(1), strip_comments(m.group(2)).strip()
            if val and name not in self.defs:
                self.defs[name] = val
        # enums
        for m in re.finditer(r"enum\s*\w*\s*\{(.*?)\}", text, re.S):
            cur = -1
            for part in m.group(1).split(","):
                part = part.strip()
                if not part:
                    continue
                if "=" in part:
                    n, v = part.split("=", 1)
                    n = n.strip()
                    try:
                        cur = self.eval(v.strip())
                    except Exception:
                        continue
                else:
                    n = part
                    cur += 1
                if re.fullmatch(r"[A-Za-z_]\w*", n):
                    self.vals[n] = cur

    def eval(self, expr: str, depth: int = 0) -> int:
        expr = expr.strip()
        if depth > 40:
            raise ValueError(expr)

        def name(m):
            n = m.group(0)
            if n in VERSIONS:
                return str(VERSIONS[n])
            if n in self.vals:
                return str(self.vals[n])
            if n in self.defs:
                return "(" + str(self.eval(self.defs[n], depth + 1)) + ")"
            raise KeyError(n)

        py = re.sub(r"\b0x[0-9a-fA-F]+[uUlL]*\b", lambda m: str(int(m.group(0).rstrip("uUlL"), 16)), expr)
        py = re.sub(r"\b(\d+)[uUlL]+\b", r"\1", py)
        py = re.sub(r"(?<![\w.])[A-Za-z_]\w*", name, py)
        py = c_ternary(py)
        py = py.replace("&&", " and ").replace("||", " or ")
        py = re.sub(r"!(?!=)", " not ", py)
        py = py.replace("/", "//")
        v = eval(py)
        return int(v)


def preprocess_keep_defines(text: str) -> str:
    """Like preprocess() but keeps #define lines in active regions."""
    env = dict(VERSIONS)
    out = []
    stack = []
    active = True
    for line in text.replace("\\\n", " ").split("\n"):
        s = line.strip()
        m = re.match(r"#\s*(if|ifdef|ifndef|elif|else|endif)\b(.*)", s)
        if m:
            d, rest = m.group(1), strip_comments(m.group(2)).strip()
            if d in ("if", "ifdef", "ifndef"):
                if d == "if":
                    c = eval_cond(rest, env) if active else False
                elif d == "ifdef":
                    c = rest in env
                else:
                    c = rest not in env
                stack.append((c, c, active))
                active = active and c
            elif d == "elif":
                cur, taken, parent = stack.pop()
                c = (not taken) and parent and eval_cond(rest, env)
                stack.append((c, taken or c, parent))
                active = parent and c
            elif d == "else":
                cur, taken, parent = stack.pop()
                stack.append((not taken, True, parent))
                active = parent and not taken
            else:
                _, _, parent = stack.pop()
                active = parent
            continue
        if active:
            out.append(line)
    return "\n".join(out)


def c_ternary(py: str) -> str:
    """Rewrite `a ? b : c` (innermost-first, parenthesised) into Python."""
    for _ in range(10):
        m = re.search(r"\(([^()?]*)\?([^():]*):([^()]*)\)", py)
        if not m:
            break
        py = py[: m.start()] + f"(({m.group(2)}) if ({m.group(1)}) else ({m.group(3)}))" + py[m.end():]
    if "?" in py:
        m = re.match(r"(.*)\?(.*):(.*)", py)
        py = f"(({m.group(2)}) if ({m.group(1)}) else ({m.group(3)}))"
    return py


# --------------------------------------------------------------------------
# Initialiser parsing
# --------------------------------------------------------------------------

def split_top(s: str, sep: str = ",") -> list[str]:
    out, depth, cur = [], 0, []
    for ch in s:
        if ch in "({[":
            depth += 1
        elif ch in ")}]":
            depth -= 1
        if ch == sep and depth == 0:
            out.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    tail = "".join(cur).strip()
    if tail:
        out.append(tail)
    return out


def find_initialisers(text: str, kind: str) -> dict[str, str]:
    """{name: body} for `<kind> name[] = { body };` / `<kind> name = { body };`."""
    out = {}
    pat = re.compile(r"(?:const\s+)?" + re.escape(kind) + r"\b\s*\*?\s*(\w+)\s*(\[[^\]]*\])?\s*=\s*\{", re.S)
    for m in pat.finditer(text):
        i = m.end()
        depth = 1
        j = i
        while depth:
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
            j += 1
        out[m.group(1)] = text[i: j - 1]
    return out


def entries(body: str) -> list[list[str]]:
    """Top-level `{ a, b, ... }` groups of an array initialiser, split into fields."""
    res = []
    for part in split_top(body):
        part = part.strip()
        if part.startswith("{") and part.endswith("}"):
            res.append(split_top(part[1:-1]))
    return res


def upper_snake(name: str) -> str:
    n = re.sub(r"^g_", "", name)
    n = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", n)
    n = re.sub(r"(?<=[A-Z])(?=[A-Z][a-z])", "_", n)
    return "G_" + n.upper()


LANG_RE = re.compile(r"^L_([A-Z0-9]+)_(\d+)$")
BANKS: set[str] = set()


def text_id(tok: str) -> str | None:
    m = LANG_RE.match(tok.strip())
    if not m:
        return None
    BANKS.add(m.group(1).lower())
    return f"tx(B_{m.group(1)}, {int(m.group(2))})"


# --------------------------------------------------------------------------
# Menus
# --------------------------------------------------------------------------

# Dialogs the Combat Simulator references that live outside the ported files
# (4MB variants, the file manager, solo-mission menus). Each maps to a stub in
# `pd_menu::stubs` so the tables stay verbatim; the call sites say what PD does.
EXTERNAL_DIALOGS = {
    "g_ChangeAgentMenuDialog": "STUB_NOT_IN_SPIKE_DIALOG",
    "g_CiOptionsViaPcMenuDialog": None,
}

#: The tables of `mplayer/ingame.c` a Combat Simulator match uses.
INGAME_MENUS = {
    "g_MpEndGameMenuItems", "g_MpEndGameMenuDialog",
    "g_MpPauseControlMenuItems", "g_MpPauseControlMenuDialog",
    "g_Mp2PMissionInventoryMenuItems", "g_MpPauseInventoryMenuDialog",
    "g_MpInGamePlayerStatsMenuItems", "g_MpPausePlayerStatsMenuDialog", "g_MpEndscreenPlayerStatsMenuDialog",
    "g_MpPlayerRankingMenuItems", "g_MpPausePlayerRankingMenuDialog", "g_MpEndscreenPlayerRankingMenuDialog",
    "g_MpTeamRankingsMenuItems", "g_MpPauseTeamRankingsMenuDialog", "g_MpEndscreenTeamRankingMenuDialog",
    "g_MpGameOverMenuItems", "g_MpEndscreenIndGameOverMenuDialog", "g_MpEndscreenTeamGameOverMenuDialog",
    "g_MpEndscreenChallengeCompletedMenuDialog", "g_MpEndscreenChallengeCheatedMenuDialog",
    "g_MpEndscreenChallengeFailedMenuDialog",
    "g_MpEndscreenConfirmNameMenuItems", "g_MpEndscreenConfirmNameMenuDialog",
    "g_MpEndscreenSavePlayerMenuItems", "g_MpEndscreenSavePlayerMenuDialog",
}

ITEM_TEXT_FNS: set[str] = set()
DIALOG_TEXT_FNS: set[str] = set()
ITEM_HANDLERS: set[str] = set()
DIALOG_HANDLERS: set[str] = set()


class MenuGen:
    def __init__(self, consts: Consts):
        self.c = consts
        self.items: dict[str, list[list[str]]] = {}
        self.dialogs: dict[str, list[str]] = {}
        self.order: list[tuple[str, str]] = []

    def add_file(self, path: str, only: set[str] | None = None):
        text = preprocess(strip_comments(read(path)))
        for name, body in find_initialisers(text, "struct menuitem").items():
            if only and name not in only:
                continue
            self.items[name] = entries(body)
            self.order.append(("items", name))
        for name, body in find_initialisers(text, "struct menudialogdef").items():
            if only and name not in only:
                continue
            self.dialogs[name] = split_top(body)
            self.order.append(("dialog", name))

    def num(self, tok: str) -> str:
        tok = tok.strip()
        try:
            v = self.c.eval(tok)
        except Exception as e:
            raise SystemExit(f"pd_menu_gen: cannot evaluate {tok!r}: {e}")
        if re.fullmatch(r"-?\d+|0x[0-9a-fA-F]+", tok):
            return str(v)
        return f"{v} /* {tok} */"

    def flags(self, tok: str) -> str:
        tok = tok.strip()
        # `VERSION >= VERSION_JPN_FINAL ? MENUDIALOGFLAG_LESSHEIGHT : 0`
        # (ingame.c): the NTSC-final side of a version ternary.
        m = re.fullmatch(r"(VERSION\s*[<>=!]=?\s*\w+)\s*\?\s*([^:]+?)\s*:\s*(.+)", tok)
        if m:
            tok = m.group(2) if eval_cond(m.group(1), dict(VERSIONS)) else m.group(3)
            tok = tok.strip()
        if tok in ("0", ""):
            return "0"
        if re.fullmatch(r"0x[0-9a-fA-F]+", tok):
            return tok
        return tok  # symbolic MENUITEMFLAG_* / MENUDIALOGFLAG_* — defined in pd_menu::types

    def param(self, tok: str, dialog_title: bool = False) -> str:
        tok = tok.strip()
        t = text_id(tok)
        if t:
            return f"P::Text({t})"
        m = re.fullmatch(r"\(uintptr_t\)\s*&\s*(\w+)", tok)
        if m:
            if dialog_title:
                # PD casts the same `char *(*)(void *)` into titles and items;
                # Rust needs the two signatures apart, so titles get `title_`.
                DIALOG_TEXT_FNS.add(m.group(1))
                return f"P::DFn(h::title_{m.group(1)})"
            ITEM_TEXT_FNS.add(m.group(1))
            return f"P::Fn(h::{m.group(1)})"
        if tok in ("0", "NULL"):
            return "P::Num(0)"
        return f"P::Num({self.num(tok)})"

    def handler(self, tok: str) -> str:
        tok = tok.strip()
        if tok in ("NULL", "0", ""):
            return "H::None"
        m = re.fullmatch(r"\(void\s*\*\)\s*&\s*(\w+)", tok)
        if m:
            return f"H::Dialog(&{self.dialog_ref(m.group(1))})"
        ITEM_HANDLERS.add(tok)
        return f"H::Fn(h::{tok})"

    def dialog_ref(self, name: str) -> str:
        if name in EXTERNAL_DIALOGS and EXTERNAL_DIALOGS[name]:
            return "super::stubs::" + EXTERNAL_DIALOGS[name]
        return upper_snake(name)

    def emit(self) -> str:
        out = []
        for kind, name in self.order:
            if kind == "items":
                es = self.items[name]
                out.append(f"pub static {upper_snake(name)}: [MenuItem; {len(es)}] = [")
                for e in es:
                    e = e + ["0"] * (6 - len(e))
                    if e[0].strip() == "MENUITEMTYPE_END":
                        out.append("    MenuItem::END,")
                        continue
                    ty, param, flags, p2, p3, hd = e[:6]
                    out.append(
                        "    MenuItem { ty: %s, param: %s, flags: %s, param2: %s, param3: %s, handler: %s },"
                        % (ty.strip(), self.num(param), self.flags(flags), self.param(p2), self.param(p3), self.handler(hd))
                    )
                out.append("];")
                out.append("")
            else:
                f = self.dialogs[name] + ["NULL"] * (6 - len(self.dialogs[name]))
                ty, title, items, hd, flags, sib = f[:6]
                hd = hd.strip()
                if hd in ("NULL", "0"):
                    hds = "None"
                else:
                    DIALOG_HANDLERS.add(hd)
                    hds = f"Some(h::{hd})"
                sib = sib.strip()
                if sib in ("NULL", "0"):
                    sibs = "None"
                else:
                    m = re.fullmatch(r"&\s*(\w+)", sib)
                    ref = self.dialog_ref(m.group(1))
                    sibs = f"Some(&{ref})" if EXTERNAL_DIALOGS.get(m.group(1), "x") is not None else "None"
                items_ref = f"&{upper_snake(items.strip())}"
                out.append(f"pub static {upper_snake(name)}: MenuDialogDef = MenuDialogDef {{")
                out.append(f"    name: \"{name}\",")
                out.append(f"    ty: {ty.strip()},")
                out.append(f"    title: {self.param(title, dialog_title=True)},")
                out.append(f"    items: {items_ref},")
                out.append(f"    handler: {hds},")
                out.append(f"    flags: {self.flags(flags)},")
                out.append(f"    nextsibling: {sibs},")
                out.append("};")
                out.append("")
        return "\n".join(out)


# --------------------------------------------------------------------------
# Data tables
# --------------------------------------------------------------------------

def gen_tables(c: Consts) -> str:
    out = []
    mp = preprocess(strip_comments(read(os.path.join(SRC, "game", "mplayer", "mplayer.c"))))
    setup = preprocess(strip_comments(read(os.path.join(SRC, "game", "mplayer", "setup.c"))))
    scen = preprocess(strip_comments(read(os.path.join(SRC, "game", "mplayer", "scenarios.c"))))
    chal = preprocess(strip_comments(read(os.path.join(SRC, "game", "challenge.c"))))
    robot = preprocess(strip_comments(read(os.path.join(SRC, "game", "modeldata", "robot.c"))))
    inv = preprocess(strip_comments(read(os.path.join(SRC, "game", "invitems.c"))))

    def ev(tok):
        tok = tok.strip()
        t = text_id(tok)
        if t:
            return t
        return str(c.eval(tok))

    def rows(text, kind, name):
        return entries(find_initialisers(text, kind)[name])

    def flat(text, kind, name):
        return split_top(find_initialisers(text, kind)[name])

    # g_MpArenas: stagenum, requirefeature, name
    out.append("pub static MP_ARENAS: &[MpArena] = &[")
    for r in rows(setup, "struct mparena", "g_MpArenas"):
        out.append(f"    MpArena {{ stagenum: {ev(r[0])}, requirefeature: {ev(r[1])}, name: {ev(r[2])} }},")
    out.append("];\n")

    out.append("pub static MP_HEADS: &[MpHead] = &[")
    for r in rows(mp, "struct mphead", "g_MpHeads"):
        out.append(f"    MpHead {{ headnum: {ev(r[0])}, requirefeature: {ev(r[1])} }},")
    out.append("];\n")

    out.append("pub static MP_BEAU_HEADS: &[MpHead] = &[")
    for r in rows(mp, "struct mphead", "g_MpBeauHeads"):
        out.append(f"    MpHead {{ headnum: {ev(r[0])}, requirefeature: {ev(r[1])} }},")
    out.append("];\n")

    for cname, rname in (("g_BotHeads", "BOT_HEADS"), ("g_MpMaleHeads", "MP_MALE_HEADS"), ("g_MpFemaleHeads", "MP_FEMALE_HEADS")):
        vals = [ev(v) for v in flat(mp, "u32", cname)]
        out.append(f"pub static {rname}: &[i32] = &[{', '.join(vals)}];\n")

    out.append("pub static BOT_PROFILES: &[BotProfile] = &[")
    for r in rows(mp, "struct botprofile", "g_BotProfiles"):
        out.append(f"    BotProfile {{ ty: {ev(r[0])}, difficulty: {ev(r[1])}, name: {ev(r[2])}, body: {ev(r[3])}, requirefeature: {ev(r[4])} }},")
    out.append("];\n")

    out.append("pub static MP_BODIES: &[MpBody] = &[")
    for r in rows(mp, "struct mpbody", "g_MpBodies"):
        out.append(f"    MpBody {{ bodynum: {ev(r[0])}, name: {ev(r[1])}, headnum: {ev(r[2])}, requirefeature: {ev(r[3])} }},")
    out.append("];\n")

    # g_MpWeapons: the menus and the match both read it, so it is written into
    # pd_core (MP_WEAPONS_RS) and re-exported here.
    MP_WEAPONS_ROWS.clear()
    MP_WEAPONS_ROWS.append("pub static MP_WEAPONS: &[MpWeapon] = &[")
    for r in rows(mp, "struct mpweapon", "g_MpWeapons"):
        r = r + ["0"] * (9 - len(r))
        MP_WEAPONS_ROWS.append(
            f"    MpWeapon {{ weaponnum: {ev(r[0])}, priammotype: {ev(r[1])}, priammoqty: {ev(r[2])}, secammotype: {ev(r[3])}, "
            f"secammoqty: {ev(r[4])}, hasweapon: {ev(r[5])}, unlockfeature: {ev(r[6])}, model: {ev(r[7])}, extrascale: {ev(r[8])} }},"
        )
    MP_WEAPONS_ROWS.append("];")
    out.append("pub use pd_core::mpweapons::MP_WEAPONS;\n")

    out.append("pub static MP_WEAPON_SETS: &[MpWeaponSet] = &[")
    for r in rows(mp, "struct mpweaponset", "g_MpWeaponSets"):
        lists = [split_top(x.strip()[1:-1]) for x in r[1:4]]
        slots = ", ".join(ev(v) for v in lists[0])
        req = ", ".join(ev(v) for v in (lists[1] + ["0"] * 4)[:4])
        locked = ", ".join(ev(v) for v in lists[2])
        out.append(f"    MpWeaponSet {{ name: {ev(r[0])}, slots: [{slots}], requirefeatures: [{req}], slotsiflocked: [{locked}] }},")
    out.append("];\n")

    out.append("pub static MP_TRACKS: &[MpTrack] = &[")
    for r in rows(mp, "struct mptrack", "g_MpTracks"):
        out.append(f"    MpTrack {{ musicnum: {ev(r[0])}, duration: {ev(r[1])}, name: {ev(r[2])}, unlockstage: {ev(r[3])} }},")
    out.append("];\n")

    out.append("pub static MP_PRESETS: &[MpPreset] = &[")
    for r in rows(mp, "struct mppreset", "g_MpPresets"):
        out.append(f"    MpPreset {{ name: {ev(r[0])}, confignum: {ev(r[1])} }},")
    out.append("];\n")

    out.append("pub static MP_CHALLENGES: &[ChallengeDef] = &[")
    for r in rows(chal, "struct challenge", "g_MpChallenges"):
        out.append(f"    ChallengeDef {{ name: {ev(r[0])}, confignum: {ev(r[1])} }},")
    out.append("];\n")

    out.append("pub static MP_SCENARIO_OVERVIEWS: &[MpScenarioOverview] = &[")
    for r in rows(scen, "struct mpscenariooverview", "g_MpScenarioOverviews"):
        out.append(f"    MpScenarioOverview {{ name: {ev(r[0])}, shortname: {ev(r[1])}, requirefeature: {ev(r[2])}, teamonly: {r[3].strip()} }},")
    out.append("];\n")

    # g_HeadsAndBodies: ismale, unk00_01, canvaryheight, type, height, filenum, scale, animscale, modeldef, handfilenum
    files = file_names()
    out.append("pub static HEADS_AND_BODIES: &[HeadOrBody] = &[")
    for r in rows(robot, "struct headorbody", "g_HeadsAndBodies"):
        fnum = c.eval(r[5])
        fname = files.get(fnum, "")
        out.append(
            f"    HeadOrBody {{ ismale: {ev(r[0])} != 0, unk00_01: {ev(r[1])} != 0, canvaryheight: {ev(r[2])} != 0, ty: {ev(r[3])}, height: {ev(r[4])}, filenum: {fnum}, "
            f"file: \"{fname}\", scale: {float(r[6].strip().rstrip('f'))!r}, animscale: {float(r[7].strip().rstrip('f'))!r} }},"
        )
    out.append("];\n")

    # Weapon names: g_Weapons[] order -> the invitem's `name` text id.
    weps = [w.strip().lstrip("&") for w in flat(inv, "struct weapondef", "g_Weapons")]
    raw_inv = read(os.path.join(SRC, "game", "invitems.c"))
    names = []
    for w in weps:
        m = re.search(r"struct weapondef " + re.escape(w) + r"\s*=\s*\{(.*?)\n\};", raw_inv, re.S)
        tid = "tx(B_MISC, 0)"
        if m:
            mm = re.search(r"(L_[A-Z0-9]+_\d+),\s*// name", m.group(1))
            if mm:
                tid = text_id(mm.group(1))
        names.append(tid)
    out.append("/// `g_Weapons[weaponnum]->name` (invitems.c), indexed by `WEAPON_*`.")
    out.append("pub static WEAPON_NAMES: &[Tx] = &[" + ", ".join(names) + "];\n")

    # Constants the handlers use by name.
    wanted = [
        "WEAPON_NONE", "WEAPON_MPSHIELD", "WEAPON_DISABLED", "MPWEAPON_NONE", "MPWEAPON_SHIELD", "MPWEAPON_DISABLED",
        "NUM_MPWEAPONS", "STAGE_MP_SKEDAR", "BODY_DARK_COMBAT", "BODY_DRCAROLL", "HEAD_VD",
        "MPHEAD_DARK_COMBAT", "MPBODY_DARK_COMBAT", "MPBODY_CASSANDRA", "MPBODY_CARRINGTON", "MPBODY_CILABTECH",
        "MPFEATURE_8BOTS", "MPFEATURE_WEAPON_SHIELD", "MPFEATURE_STAGE_COMPLEX", "MPFEATURE_STAGE_TEMPLE",
        "MPFEATURE_STAGE_FELICITY", "MPFEATURE_ONEHITKILLS", "MPFEATURE_SLOWMOTION", "MPFEATURE_SCENARIO_KOH",
        "MPFEATURE_SCENARIO_CTC", "MPFEATURE_SCENARIO_PAC", "MPFEATURE_STAGE_CARPARK", "MPFEATURE_BOTDIFF_DARK",
        "BOTTYPE_GENERAL", "BOTDIFF_NORMAL", "BOTDIFF_DISABLED", "NUM_BOTDIFFS",
        "MPSCENARIO_COMBAT", "MPSCENARIO_HOLDTHEBRIEFCASE", "MPSCENARIO_HACKERCENTRAL", "MPSCENARIO_POPACAP",
        "MPSCENARIO_KINGOFTHEHILL", "MPSCENARIO_CAPTURETHECASE",
        "MPOPTION_TEAMSENABLED", "MPOPTION_ONEHITKILLS", "MPOPTION_SLOWMOTION_ON", "MPOPTION_SLOWMOTION_SMART",
        "MPOPTION_DISPLAYTEAM", "MPOPTION_KILLSSCORE", "MPOPTION_HTB_HIGHLIGHTBRIEFCASE", "MPOPTION_HTB_SHOWONRADAR",
        "MPOPTION_CTC_SHOWONRADAR", "MPOPTION_KOH_HILLONRADAR", "MPOPTION_KOH_MOBILEHILL", "MPOPTION_00010000",
        "MPOPTION_HTM_HIGHLIGHTTERMINAL", "MPOPTION_HTM_SHOWONRADAR", "MPOPTION_PAC_HIGHLIGHTTARGET", "MPOPTION_PAC_SHOWONRADAR",
        "MPDISPLAYOPTION_RADAR", "MPDISPLAYOPTION_HIGHLIGHTTEAMS",
        "OPTION_LOOKAHEAD", "OPTION_SIGHTONSCREEN", "OPTION_AUTOAIM", "OPTION_AMMOONSCREEN", "OPTION_SHOWGUNFUNCTION",
        "OPTION_HEADROLL", "OPTION_0100", "OPTION_ALWAYSSHOWTARGET", "OPTION_SHOWZOOMRANGE", "OPTION_FORWARDPITCH",
        "CONTROLMODE_11", "MPPLAYERTITLE_BEGINNER", "MPPLAYERTITLE_PERFECT",
        "MPLOCKTYPE_NONE", "MPLOCKTYPE_LASTWINNER", "MPLOCKTYPE_LASTLOSER", "MPLOCKTYPE_RANDOM", "MPLOCKTYPE_CHALLENGE",
        "MPLOCKTYPE_PLAYER",
        "MPQUICKTEAM_NONE", "MPQUICKTEAM_PLAYERSONLY", "MPQUICKTEAM_PLAYERSANDSIMS", "MPQUICKTEAM_PLAYERSTEAMS",
        "MPQUICKTEAM_PLAYERSVSSIMS", "MPQUICKTEAM_PLAYERSIMTEAMS",
        "MPSETUPMENU_GENERAL", "MPSETUPMENU_ADVSETUP", "MPSETUPMENU_QUICKGO",
        "MAX_PLAYERS", "MAX_BOTS", "MAX_MPCHRS", "MAX_TEAMS", "NUM_MPWEAPONSLOTS", "MAX_USERSTRING_LEN",
        "WEAPONSET_RANDOM", "WEAPONSET_RANDOMFIVE", "WEAPONSET_CUSTOM",
        "SOLOSTAGEINDEX_SKEDARRUINS", "SLOWMOTION_OFF", "SLOWMOTION_ON", "SLOWMOTION_SMART",
        "DESCRIPTION_MPCONFIG", "DESCRIPTION_MPCHALLENGE",
        "ANIM_01FC", "ANIM_040D", "FILE_GHUDPIECE",
        "SFXNUM_05BB_MENU_SWIPE", "SFXNUM_05BC_MENU_OPENDIALOG", "SFXNUM_0441_MENU_FOCUS", "SFXNUM_05DD_MENU_SELECT",
        "SFXMAP_8040_MENU_ERROR", "SFXMAP_8098_EXPLOSION", "SFXMAP_809A_EXPLOSION", "SFXNUM_043E_MENU_SUBFOCUS",
        "SFXNUM_00EA_PICKUP_AMMO", "SFXNUM_002B_MENU_CANCEL",
        "MODELPART_HEAD_SUNGLASSES", "MODELPART_HEAD_EYESCLOSED", "MODELPART_HEAD_HUDPIECE",
    ]
    out.append("// Decomp constants the hand-ported handlers use (constants.h, NTSC-final).")
    for n in wanted:
        try:
            v = c.eval(n)
        except Exception:
            print(f"pd_menu_gen: warning: constant {n} unresolved", file=sys.stderr)
            continue
        out.append(f"pub const {n}: i32 = {v};")
    out.append("")
    return "\n".join(out)


def file_names() -> dict[int, str]:
    """FILE_* number -> ROM file name (src/assets/<version>/files order via files.h)."""
    out = {}
    for m in re.finditer(r"#define\s+FILE_(\w+)\s+(0x[0-9a-fA-F]+|\d+)", read(os.path.join(SRC, "include", "files.h"))):
        out[int(m.group(2), 0)] = m.group(1)
    return out


# --------------------------------------------------------------------------
# Assets
# --------------------------------------------------------------------------

#: The ROM fonts the Combat Simulator draws with (`text.c`): the four Handel
#: Gothic sizes (menus, HUD) and the numeric font (the ammo counter).
FONTS = ("handelgothicxs.bin", "handelgothicsm.bin", "handelgothicmd.bin", "handelgothiclg.bin", "numeric.bin")


def langbanks() -> dict[str, int]:
    """`LANGBANK_*` (include/lang.h): bank name -> number. A text id is
    `(bank << 9) | index`."""
    text = read(os.path.join(SRC, "include", "lang.h"))
    return {m.group(1).lower(): int(m.group(2), 16)
            for m in re.finditer(r"#define\s+LANGBANK_(\w+)\s+(0x[0-9a-fA-F]+)", text)}


def export_assets() -> dict:
    """fonts/, lang/en.json + lang/mpstringsE.bin, data/mpconfigs.bin."""
    os.makedirs(os.path.join(OUT_ASSETS, "fonts"), exist_ok=True)
    fonts = os.path.join(SRC, "assets", "ntsc-final", "fonts")
    for f in FONTS:
        shutil.copyfile(os.path.join(fonts, f), os.path.join(OUT_ASSETS, "fonts", f))

    # Every bank, in English: the stage banks carry MP arena strings too.
    banks = {}
    for bank, num in sorted(langbanks().items(), key=lambda kv: kv[1]):
        path = os.path.join(SRC, "assets", "ntsc-final", "lang", bank + ".json")
        rows = json.load(open(path, encoding="utf-8"))
        arr = []
        for r in rows:
            m = LANG_RE.match(r["id"])
            idx = int(m.group(2))
            while len(arr) <= idx:
                arr.append("")
            arr[idx] = r.get("en") or ""
        banks[bank] = {"langbank": num, "strings": arr}
    os.makedirs(os.path.join(OUT_ASSETS, "lang"), exist_ok=True)
    with open(os.path.join(OUT_ASSETS, "lang", "en.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump({"source": "pd-decomp/src/assets/ntsc-final/lang/*.json (en); bank numbers from include/lang.h",
                   "banks": banks}, f, ensure_ascii=False, indent=0)
        f.write("\n")

    os.makedirs(os.path.join(OUT_ASSETS, "data"), exist_ok=True)
    shutil.copyfile(os.path.join(EXTRACTED, "mpconfigs.bin"), os.path.join(OUT_ASSETS, "data", "mpconfigs.bin"))
    shutil.copyfile(os.path.join(EXTRACTED, "mpstringsE.bin"), os.path.join(OUT_ASSETS, "lang", "mpstringsE.bin"))
    return {"fonts": len(FONTS), "langbanks": len(banks)}


def write_rust(verbose: bool = False) -> dict:
    """Write `crates/pd_menu/src/generated.rs`: the menu and MP tables."""
    c = Consts()
    for h in ("constants.h", "files.h", "sfx.h"):
        c.load_header(os.path.join(SRC, "include", h))
    # MUSIC_* and ANIM_* are generated at build time from these JSON lists
    # (their index is the id).
    for js in ("sequences.json", "animations.json"):
        rows = json.load(open(os.path.join(SRC, "assets", "ntsc-final", js), encoding="utf-8"))
        for i, r in enumerate(rows):
            c.vals.setdefault(r["id"], i)
    # A few helpers the menu tables use that live in other headers.
    for extra in ("game/mplayer/scenarios.h",):
        p = os.path.join(SRC, "include", extra)
        if os.path.exists(p):
            c.load_header(p)

    mg = MenuGen(c)
    mg.add_file(os.path.join(SRC, "game", "mplayer", "setup.c"))
    for inc in ("combat.inc", "holdthebriefcase.inc", "hackthatmac.inc", "popacap.inc", "kingofthehill.inc", "capturethecase.inc"):
        mg.add_file(os.path.join(SRC, "game", "mplayer", "scenarios", inc))
    mg.add_file(os.path.join(SRC, "game", "mplayer", "scenarios.c"))
    mg.add_file(os.path.join(SRC, "game", "mainmenu.c"), only={"g_MainMenuMenuItems", "g_CiMenuViaPcMenuDialog"})
    # The pause menu and the end-of-match dialogs of a Combat Simulator match
    # (the 2-player mission ones are the solo game's).
    mg.add_file(os.path.join(SRC, "game", "mplayer", "ingame.c"), only=INGAME_MENUS)

    menus = mg.emit()
    tables = gen_tables(c)

    banks = sorted(BANKS | {"mpmenu", "mpweapons", "options", "misc", "gun"})
    head = [
        "//! GENERATED by tools/pd-assets/pd_menu_gen.py from reference/pd-decomp (NTSC-final).",
        "//! Do not edit: re-run the script. Menu tables are setup.c / scenarios.c / the scenario",
        "//! .inc files / mainmenu.c verbatim; data tables are mplayer.c, challenge.c, robot.c,",
        "//! invitems.c. Handler and text-function names are PD's; they live in `super::handlers`.",
        "#![allow(clippy::all, dead_code, non_upper_case_globals)]",
        "",
        "use super::handlers as h;",
        "use super::types::*;",
        "use pd_core::lang::{tx, Tx};",
        "",
        "// Language banks (`L_<BANK>_nnn`): PD's LANGBANK_* numbers (include/lang.h),",
        "// as pd_core::lang indexes them.",
    ]
    lb = langbanks()
    for b in banks:
        head.append(f"pub const B_{b.upper()}: u8 = {lb[b]:#04x};")
    head.append("")

    os.makedirs(os.path.dirname(OUT_RS), exist_ok=True)
    with open(OUT_RS, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(head) + "\n" + tables + "\n// ---- Menus ----\n\n" + menus + "\n")

    with open(MP_WEAPONS_RS, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join([
            "//! GENERATED by tools/pd-assets/pd_menu_gen.py from reference/pd-decomp (NTSC-final).",
            "//! Do not edit: re-run the script. `g_MpWeapons` (`mplayer.c:56`): what a Combat",
            "//! Simulator weapon slot can hold; `MPWEAPON_*` indexes it.",
            "",
            "use crate::mp::MpWeapon;",
            "",
        ] + MP_WEAPONS_ROWS) + "\n")

    print(f"pd_menu_gen: {len(mg.items)} item arrays, {len(mg.dialogs)} dialogs -> {OUT_RS}")
    if verbose:
        print("item text fns:", " ".join(sorted(ITEM_TEXT_FNS)))
        print("dialog text fns:", " ".join(sorted(DIALOG_TEXT_FNS)))
        print("item handlers:", " ".join(sorted(ITEM_HANDLERS)))
        print("dialog handlers:", " ".join(sorted(DIALOG_HANDLERS)))
    return {"menu_item_arrays": len(mg.items), "menu_dialogs": len(mg.dialogs)}


def main() -> int:
    write_rust(verbose=True)
    export_assets()
    return 0


if __name__ == "__main__":
    sys.exit(main())
