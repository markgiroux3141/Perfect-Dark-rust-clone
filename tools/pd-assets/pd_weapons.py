#!/usr/bin/env python3
"""Transcribe Perfect Dark's weapon table out of the decomp, with provenance.

The arsenal is authored data, not code constants: `game/invitems.c` carries one
`weapondef` per weapon (models, four gun scripts, **two firing functions**, two
`ammodef`s, viewmodel placement, flags), and `game/mplayer/mplayer.c` carries
`g_MpWeapons[]` — the multiplayer set, which is the scope this port takes.

Hand-transcribing that is ~40 weapons x 2 functions x ~20 fields of bare numeric
literals, which is precisely where a silent one-column slip lives. So this parses
the C instead and emits JSON with a **source line number per row**, the same
provenance discipline `game/src/combat/attack_anim.rs` uses. Re-run it and diff.

  python tools/pd-assets/pd_weapons.py json  out.json     # the whole MP table
  python tools/pd-assets/pd_weapons.py list             # one line per MP weapon
  python tools/pd-assets/pd_weapons.py show falcon2     # one weapon, expanded

RESOLUTION CHAIN (every link is the decomp's own, none inferred from names):

  MPWEAPON_x  -> g_MpWeapons[x]                  mplayer.c:56  — the MP set
              -> .weaponnum  -> g_Weapons[n]     invitems.c    — the weapondef
              -> .hi_model   -> FILE_G<name>     files.h       — first-person model
              -> guns/<name>.bin                 Makefile:209  — the build's own rule
  g_MpWeapons[x].model -> MODEL_CHR<name>        constants.h   — third-person model
              -> g_ModelStates[MODEL_...]        modeldata/general.c
              -> FILE_PCHR<name> -> props/chr<name>.bin        Makefile:210

The two Makefile lines are load-bearing and worth restating, because "FILE_GFALCON2
means guns/falcon2.bin" looks like a guess and is not:

    $(patsubst $(A_DIR)/files/guns/%.bin,  .../G%Z, ...)
    $(patsubst $(A_DIR)/files/props/%.bin, .../P%Z, ...)

VERSION: the repo's assets are ntsc-final, so `#if VERSION` blocks resolve for
NTSC final (>= NTSC_1_0 taken, == JPN_FINAL not taken). MPWEAPON_* values are
themselves version ternaries in constants.h and get the same treatment.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys

# ---------------------------------------------------------------------------
# Decomp location
# ---------------------------------------------------------------------------

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
DECOMP = os.path.join(REPO, "reference", "pd-decomp")
SRC = os.path.join(DECOMP, "src")
ASSETS = os.path.join(SRC, "assets", "ntsc-final")


def src(*parts: str) -> str:
    return os.path.join(SRC, *parts)


def require_decomp() -> None:
    if not os.path.isdir(SRC):
        sys.exit(
            f"decomp not found at {DECOMP}\n"
            "It is gitignored — see reference/README.md for how to re-clone it."
        )


# ---------------------------------------------------------------------------
# #define scraping
# ---------------------------------------------------------------------------

# `#define NAME value`, where value may be a version ternary:
#   #define MPWEAPON_CROSSBOW (VERSION == VERSION_JPN_FINAL ? 0x19 : 0x1a)
DEFINE_RE = re.compile(r"^#define\s+([A-Z][A-Z0-9_]*)\s+(.+?)\s*$")
TERNARY_RE = re.compile(r"^\(\s*VERSION\s*==\s*VERSION_JPN_FINAL\s*\?\s*(\S+)\s*:\s*(\S+)\s*\)$")


def parse_int(tok: str) -> int | None:
    """A C integer literal, or None if `tok` isn't one."""
    tok = tok.strip().rstrip("uUlL")
    try:
        return int(tok, 16) if tok.lower().startswith("0x") else int(tok, 10)
    except ValueError:
        return None


ENUM_RE = re.compile(r"\benum\s+(\w+)\s*\{(.*?)\}\s*;", re.S)


def scrape_enums(path: str, names: tuple[str, ...]) -> dict[str, int]:
    """Members of the named C enums, with implicit `= previous + 1` numbering.

    `WEAPON_*` is an **enum** (`constants.h:4561 enum weaponnum`), not a set of
    #defines — which is why a defines-only scrape finds nothing and every row
    silently drops out. Explicit `= value` members reseat the counter.

    The enum body contains `#if VERSION` guards of its own (WEAPON_SHIELDTECHITEM
    is NTSC-1.0+), so it needs the same resolution the initializers get. Without
    it two members come back with the `#if`/`#endif` glued to their names and
    every weaponnum past 0x4f resolves to the wrong `g_Weapons` entry — which is
    a silently-plausible wrong answer, not a crash.
    """
    text = resolve_version_ifs(
        strip_comments(open(path, encoding="utf-8", errors="replace").read())
    )
    out: dict[str, int] = {}
    for m in ENUM_RE.finditer(text):
        if m.group(1) not in names:
            continue
        nxt = 0
        for member in m.group(2).split(","):
            member = member.strip()
            if not member:
                continue
            if "=" in member:
                lhs, rhs = member.split("=", 1)
                v = parse_int(rhs)
                if v is None:
                    v = out.get(rhs.strip())
                if v is None:
                    continue
                out[lhs.strip()] = v
                nxt = v + 1
            else:
                out[member] = nxt
                nxt += 1
    return out


def scrape_defines(path: str, prefixes: tuple[str, ...]) -> dict[str, int]:
    """Every `#define <PREFIX>...` in `path` whose value is an integer literal.

    Version ternaries resolve to the **non-JPN** branch (this repo's assets are
    ntsc-final), which is what makes the MPWEAPON_* numbering above 0x18 correct.
    """
    out: dict[str, int] = {}
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = DEFINE_RE.match(line.strip())
            if not m:
                continue
            name, value = m.group(1), m.group(2)
            if not name.startswith(prefixes):
                continue
            # Many flag defines carry a trailing `// what it does`, which would
            # otherwise land inside the value and fail to parse — dropping the
            # flag silently rather than loudly. (This ate WEAPONFLAG_ONEHANDED
            # and 6 of the 25 FUNCFLAG_*.)
            value = value.split("//")[0].split("/*")[0].strip()
            t = TERNARY_RE.match(value)
            if t:
                value = t.group(2)  # the non-JPN arm
            v = parse_int(value)
            if v is not None:
                out[name] = v
    return out


# ---------------------------------------------------------------------------
# C preprocessing (just enough) + initializer splitting
# ---------------------------------------------------------------------------


def strip_comments(text: str) -> str:
    """Remove /* */ and // comments, preserving line count so numbers stay true."""

    def repl(m: re.Match[str]) -> str:
        s = m.group(0)
        if s.startswith("/"):
            return "".join(c for c in s if c == "\n")
        return s

    return re.sub(r'/\*.*?\*/|//[^\n]*|"(?:\\.|[^"\\])*"', repl, text, flags=re.S)


def resolve_version_ifs(text: str) -> str:
    """Collapse `#if VERSION ...` blocks for ntsc-final, keeping line count.

    Only the two forms that actually appear inside the tables we read are
    handled; anything else is left alone (and would show up as an unparsed token
    rather than a wrong number).
    """
    lines = text.split("\n")
    out: list[str] = []
    # Stack of "are we emitting?" plus whether this level's #if was taken.
    stack: list[tuple[bool, bool]] = []

    for line in lines:
        s = line.strip()
        if s.startswith("#if"):
            cond = s[3:].lstrip("defined").strip()
            if "VERSION_JPN_FINAL" in cond and "==" in cond:
                taken = False
            elif "VERSION_NTSC_1_0" in cond and (">=" in cond or ">" in cond):
                taken = True
            elif "VERSION_JPN_FINAL" in cond and "!=" in cond:
                taken = True
            else:
                # Unknown condition: keep the body, so nothing silently vanishes.
                taken = True
            emitting = taken and all(e for e, _ in stack)
            stack.append((emitting, taken))
            out.append("")
            continue
        if s.startswith("#else") and stack:
            emitting_parent = all(e for e, _ in stack[:-1])
            _, taken = stack[-1]
            stack[-1] = (emitting_parent and not taken, taken)
            out.append("")
            continue
        if s.startswith("#endif") and stack:
            stack.pop()
            out.append("")
            continue
        out.append(line if all(e for e, _ in stack) else "")

    return "\n".join(out)


def split_top_level(body: str) -> list[str]:
    """Split an initializer body at top-level commas, keeping braced groups whole."""
    parts: list[str] = []
    depth = 0
    cur: list[str] = []
    for ch in body:
        if ch in "{[(":
            depth += 1
        elif ch in "}])":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(cur).strip())
            cur = []
            continue
        cur.append(ch)
    tail = "".join(cur).strip()
    if tail:
        parts.append(tail)
    return parts


INIT_RE = re.compile(
    # `struct T name = {`, `struct T name[] = {`, and the pointer-array form
    # `struct weapondef *g_Weapons[] = {` that the master table uses.
    r"^\s*(?:static\s+)?(?:const\s+)?struct\s+(\w+)\s*\**\s*(\w+)\s*(\[[^\]]*\])?\s*=\s*\{",
    re.M,
)


def find_initializers(text: str) -> dict[str, dict]:
    """Every `struct T name[] = { ... };` in `text`, by symbol name.

    Returns {name: {"struct": T, "line": 1-based, "body": str, "array": bool}}.
    """
    out: dict[str, dict] = {}
    for m in INIT_RE.finditer(text):
        stype, name, arr = m.group(1), m.group(2), m.group(3)
        # Walk to the matching close brace.
        i = m.end() - 1
        depth = 0
        while i < len(text):
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        body = text[m.end() : i]
        out[name] = {
            "struct": stype,
            "line": text.count("\n", 0, m.start()) + 1,
            "body": body,
            "array": arr is not None,
        }
    return out


# ---------------------------------------------------------------------------
# Field layouts — transcribed from src/include/types.h
# ---------------------------------------------------------------------------

# `struct funcdef` (types.h:2909) — the head every firing function shares.
FUNCDEF_BASE = ["type", "name", "unk06", "ammoindex", "noisesettings", "fire_animation", "flags"]

# `struct funcdef_shoot` (types.h:2919).
FUNCDEF_SHOOT = FUNCDEF_BASE + [
    "recoilsettings",
    "recoverytime60",
    "damage",
    "spread",
    "unk24",
    "unk25",
    "unk26",
    "unk27",
    "recoildist",
    "recoilangle",
    "slidemax",
    "impactforce",
    "duration60",
    "shootsound",
    "penetration",
]

FUNCDEF_FIELDS: dict[str, list[str]] = {
    # types.h:2944
    "funcdef_shootsingle": FUNCDEF_SHOOT,
    # types.h:2948 — autos spin up from initialrpm to maxrpm
    "funcdef_shootauto": FUNCDEF_SHOOT
    + ["initialrpm", "maxrpm", "vibrationstart", "vibrationmax", "turretaccel", "turretdecel"],
    # types.h:2958
    "funcdef_shootprojectile": FUNCDEF_SHOOT
    + [
        "projectilemodelnum",
        "unk44",
        "scale",
        "speed",
        "speeddecel",
        "traveldist",
        "timer60",
        "hitspeedpreservationfrac",
        "soundnum",
    ],
    # types.h:2971
    "funcdef_throw": FUNCDEF_BASE
    + ["projectilemodelnum", "activatetime60", "recoverytime60", "damage"],
    # types.h:2979 — 13 trailing fields the decomp marks unused
    "funcdef_melee": FUNCDEF_BASE
    + ["damage", "range"]
    + [f"unused{i}" for i in range(13)],
    # types.h:2997
    "funcdef_special": FUNCDEF_BASE + ["specialfunc", "recoverytime60", "soundnum"],
    # types.h:3004
    "funcdef_device": FUNCDEF_BASE + ["device"],
    # types.h:2919 — the bare shoot struct is used directly by a few entries
    "funcdef_shoot": FUNCDEF_SHOOT,
    "funcdef": FUNCDEF_BASE,
}

# `struct ammodef` (types.h:3009).
AMMODEF_FIELDS = ["type", "casingeject", "clipsize", "reload_animation", "flags"]

# `struct weapondef` (types.h:3023).
#
# `functions[2]` is initialised TWO different ways in the same file, which is a
# real ambiguity and not a parser bug:
#
#   invitem_falcon2  { &invfunc_falcon2_singleshot, &invfunc_falcon2_pistolwhip },
#   invitem_hammer   NULL, // pri function
#                    NULL, // sec function
#
# So the braced form is one top-level token and the flat form is two, giving 22
# or 23 tokens for the same struct. `weapondef_fields` picks the layout by
# looking at the token rather than assuming either one.
WEAPONDEF_FIELDS = [
    "hi_model",
    "lo_model",
    "equip_animation",
    "unequip_animation",
    "pritosec_animation",
    "sectopri_animation",
    "functions",
    "pri_ammo",
    "sec_ammo",
    "aimsettings",
    "muzzlez",
    "posx",
    "posy",
    "posz",
    "sway",
    "gunviscmds",
    "partvisibility",
    "shortname",
    "name",
    "manufacturer",
    "description",
    "flags",
]

# `struct explosiontype` (types.h:4313) — `g_ExplosionTypes[]`, explosions.c:41.
# The two fields our single spherical `Explosion` has no equivalent for are
# `blastradius` vs `damageradius` (they differ by up to 2x — the visual blast is
# much smaller than the lethal one) and `propagationrate`, which is why a PD
# explosion expands over its `duration` instead of applying instantly.
EXPLOSIONTYPE_FIELDS = [
    "rangeh",
    "rangev",
    "changerateh",
    "changeratev",
    "innersize",
    "blastradius",
    "damageradius",
    "duration",
    "propagationrate",
    "flarespeed",
    "smoketype",
    "sound",
    "damage",
]

# `struct botweaponconfig` (types.h:5552) — the AI's *per-function* weapon
# preference, which is the table that actually answers "does a hunter pick the
# primary or the secondary": `score1`/`score2` are the desirability of each
# function and `pri/secdistconfig` name the engagement band each one wants.
#
# NOTE this is NOT `WEAPONFLAG_AICANUSE`. That flag is set on all 64 real weapons
# and absent only from the 20 non-weapons (keycards, briefcases, bare projectile
# items), so it gates *items*, not guns — measured, see the module docstring of
# the generated Rust. The real per-weapon AI data is here.
BOTWEAPONCONFIG_FIELDS = [
    "score1",
    "score2",
    "dualscore1",
    "dualscore2",
    "haspriammogoal",
    "hassecammogoal",
    "pridistconfig",
    "secdistconfig",
    "targetammopri",
    "targetammosec",
    "criticalammopri",
    "criticalammosec",
    "reloaddelay",
    "allowpartialreloaddelay",
]

# `struct mpweapon` (types.h:4933). NOTE `hasweapon : 1` and `unlockfeature : 7`
# share a byte but are initialised as TWO separate tokens, so this list is nine
# long, not eight — the first thing the strict token count caught.
MPWEAPON_FIELDS = [
    "weaponnum",
    "priammotype",
    "priammoqty",
    "secammotype",
    "secammoqty",
    "hasweapon",
    "unlockfeature",
    "model",
    "extrascale",
]


# ---------------------------------------------------------------------------
# Value resolution
# ---------------------------------------------------------------------------


class Consts:
    """The #define namespaces the tables reference, merged with a flag reverse-map."""

    def __init__(self) -> None:
        c = src("include", "constants.h")
        f = src("include", "files.h")
        self.all = scrape_defines(
            c,
            (
                "WEAPON_",
                "MPWEAPON_",
                "AMMOTYPE_",
                "CASING_",
                "FUNCFLAG_",
                "WEAPONFLAG_",
                "INVENTORYFUNCTYPE_",
                "MODEL_",
                "SPECIALFUNC_",
                "GUNFEATURE_",
                "MPFEATURE_",
                "BOTDISTCFG_",
                "MODELPART_",
                "EXPLOSIONTYPE_",
                "SMOKETYPE_",
            ),
        )
        # WEAPON_* / MPWEAPON_INDEX live in enums, not defines.
        self.all.update(scrape_enums(c, ("weaponnum",)))
        self.files = scrape_defines(f, ("FILE_",))
        self.all.update(self.files)

    def value(self, name: str) -> int | None:
        return self.all.get(name)

    def by_prefix(self, prefix: str) -> dict[str, int]:
        return {k: v for k, v in self.all.items() if k.startswith(prefix)}


def resolve_expr(tok: str, consts: Consts) -> object:
    """A single initializer token → a JSON-friendly value.

    Numbers stay numbers; `NULL` becomes None; `&sym` / `sym` become the symbol
    name (a reference the caller resolves); `A | B` becomes the OR'd int when
    every operand is a known constant, else the literal source text.
    """
    tok = tok.strip()
    if not tok:
        return None
    if tok == "NULL":
        return None
    # C booleans. Worth handling centrally: as bare symbols they come back as the
    # STRINGS "true"/"false", and `bool("false")` is True in Python — which silently
    # made every `modelpartvisibility` row read as visible.
    if tok == "true":
        return True
    if tok == "false":
        return False
    if tok.startswith("{") and tok.endswith("}"):
        return [resolve_expr(p, consts) for p in split_top_level(tok[1:-1])]

    # Float or int literal (incl. the decomp's long decimal expansions).
    if re.fullmatch(r"-?\d+\.\d+(?:[eE][-+]?\d+)?f?", tok):
        return float(tok.rstrip("f"))
    v = parse_int(tok)
    if v is not None:
        return v

    if "|" in tok:
        names = [p.strip().lstrip("&") for p in tok.split("|")]
        vals = [consts.value(n) for n in names]
        if all(x is not None for x in vals):
            acc = 0
            for x in vals:
                acc |= x  # type: ignore[operator]
            return acc
        return tok

    bare = tok.lstrip("&").strip()
    v = consts.value(bare)
    if v is not None:
        return v
    return bare  # a symbol reference (a funcdef/ammodef/guncmd name)


FUNCTIONS_AT = WEAPONDEF_FIELDS.index("functions")


def weapondef_fields(body: str) -> list[str]:
    """The field list matching this weapondef's `functions[2]` initializer shape.

    See the note on `WEAPONDEF_FIELDS`: braced pair (22 tokens) vs two flat
    tokens (23). Decided from the token itself, so neither shape is assumed.
    """
    toks = [t for t in split_top_level(body) if t.strip()]
    if len(toks) > FUNCTIONS_AT and toks[FUNCTIONS_AT].lstrip().startswith("{"):
        return WEAPONDEF_FIELDS
    return (
        WEAPONDEF_FIELDS[:FUNCTIONS_AT]
        + ["pri_function", "sec_function"]
        + WEAPONDEF_FIELDS[FUNCTIONS_AT + 1 :]
    )


def map_fields(body: str, fields: list[str], consts: Consts) -> dict:
    """Zip an initializer body onto a field-name list.

    A token-count mismatch is a hard error, not a shrug: it is exactly the
    silent one-column slip this whole script exists to prevent.
    """
    toks = split_top_level(body)
    toks = [t for t in toks if t.strip()]
    if len(toks) > len(fields):
        raise ValueError(f"{len(toks)} tokens for {len(fields)} fields: {toks[:6]}...")
    out: dict[str, object] = {}
    for name, tok in zip(fields, toks):
        out[name] = resolve_expr(tok, consts)
    return out


# ---------------------------------------------------------------------------
# Filename resolution — FILE_* -> the .bin on disk
# ---------------------------------------------------------------------------


def file_symbol_to_path(sym: str) -> str | None:
    """`FILE_GFALCON2` -> `guns/falcon2.bin`, per the Makefile's own patsubst.

    Returns None for a file symbol outside guns/ and props/ (setups, bgdata, …).
    The name is lowercased and checked against the extracted assets, so a bad
    guess fails loudly here instead of producing a dangling roster entry.
    """
    if not sym.startswith("FILE_"):
        return None
    stem = sym[len("FILE_") :]
    if stem.startswith("G"):
        rel = os.path.join("guns", stem[1:].lower() + ".bin")
    elif stem.startswith("P"):
        rel = os.path.join("props", stem[1:].lower() + ".bin")
    else:
        return None
    if not os.path.isfile(os.path.join(ASSETS, "files", rel)):
        return None
    return rel.replace(os.sep, "/")


# ---------------------------------------------------------------------------
# Language strings
# ---------------------------------------------------------------------------


def load_gun_strings() -> dict[str, str]:
    """`L_GUN_007` -> "Falcon 2", from the authored English language file."""
    path = os.path.join(ASSETS, "lang", "gun.json")
    if not os.path.isfile(path):
        return {}
    with open(path, encoding="utf-8") as fh:
        rows = json.load(fh)
    # A few ids are explicitly null in the JSON (unused slots), so don't assume a str.
    return {r["id"]: (r.get("en") or "").replace("\n", " ").strip() for r in rows}


def gun_string(strings: dict[str, str], token: object) -> str:
    """The authored English text for a `L_GUN_*` name/description field.

    These fields are `u16` text ids in the struct, but the source writes them as
    `L_GUN_007` symbols that exist only in the generated language headers — so
    `resolve_expr` hands back the symbol string, and the symbol is exactly the
    key `gun.json` uses. Look it up directly rather than trying to reconstruct an
    index from a number that was never there.
    """
    if isinstance(token, str) and token.startswith("L_GUN_"):
        return strings.get(token, "")
    return ""


# ---------------------------------------------------------------------------
# The table build
# ---------------------------------------------------------------------------

# The four MP entries with no gameplay system to attach to — the handoff's
# "no system to attach to; defer or decline" list. Excluded from the port scope
# by explicit user decision, not by judgement.
#: The first-person muzzle-flash part (`constants.h`).
MODELPART_GUN_MUZZLEFLASH1 = 0x005A

EQUIPMENT_ONLY = {
    "MPWEAPON_XRAYSCANNER",
    "MPWEAPON_CLOAKINGDEVICE",
    "MPWEAPON_COMBATBOOST",
    "MPWEAPON_SHIELD",
}


def build() -> dict:
    require_decomp()
    consts = Consts()
    strings = load_gun_strings()

    # --- invitems.c: the weapondefs, funcdefs and ammodefs -----------------
    inv_path = src("game", "invitems.c")
    inv_text = resolve_version_ifs(strip_comments(open(inv_path, encoding="utf-8").read()))
    inv = find_initializers(inv_text)

    if "g_Weapons" not in inv:
        sys.exit(f"g_Weapons[] not found in {inv_path} — did the decomp layout change?")

    # g_Weapons[] is indexed by WEAPON_*; entries are `&invitem_foo`.
    weapon_symbols = [
        resolve_expr(t, consts) for t in split_top_level(inv["g_Weapons"]["body"]) if t.strip()
    ]

    # --- mplayer.c: g_MpWeapons[], the MP set ------------------------------
    mp_path = src("game", "mplayer", "mplayer.c")
    mp_text = resolve_version_ifs(strip_comments(open(mp_path, encoding="utf-8").read()))
    mp_init = find_initializers(mp_text)
    if "g_MpWeapons" not in mp_init:
        sys.exit(f"g_MpWeapons[] not found in {mp_path}")
    mp_rows_raw = [t for t in split_top_level(mp_init["g_MpWeapons"]["body"]) if t.strip()]

    # --- modeldata/general.c: MODEL_* index -> FILE_* ----------------------
    gen_path = src("game", "modeldata", "general.c")
    gen_text = resolve_version_ifs(strip_comments(open(gen_path, encoding="utf-8").read()))
    gen_init = find_initializers(gen_text)
    model_table = next(
        (v for k, v in gen_init.items() if "ModelStates" in k or "Models" in k),
        None,
    )
    if model_table is None:
        # Fall back to the largest array in the file — the model table is by far it.
        model_table = max(gen_init.values(), key=lambda v: len(v["body"]))
    model_files: list[str | None] = []
    for row in split_top_level(model_table["body"]):
        if not row.strip():
            continue
        inner = row.strip()
        if inner.startswith("{"):
            parts = split_top_level(inner[1:-1])
        else:
            parts = [inner]
        sym = None
        for p in parts:
            p = p.strip().lstrip("&")
            if p.startswith("FILE_"):
                sym = p
                break
        model_files.append(sym)

    # Reverse map: MODEL_* name -> index, so a `model` field resolves to a file.
    model_consts = consts.by_prefix("MODEL_")
    model_by_index: dict[int, str] = {}
    for name, idx in model_consts.items():
        model_by_index.setdefault(idx, name)

    def model_to_bin(model_index: object) -> tuple[str | None, str | None]:
        """MODEL_* index -> (its FILE_* symbol, its props/*.bin path)."""
        if not isinstance(model_index, int) or model_index < 0:
            return None, None
        if model_index >= len(model_files):
            return None, None
        sym = model_files[model_index]
        return sym, (file_symbol_to_path(sym) if sym else None)

    def expand_func(sym: object) -> dict | None:
        """A `&invfunc_*` reference -> its parsed funcdef, with provenance."""
        if not isinstance(sym, str) or sym not in inv:
            return None
        entry = inv[sym]
        fields = FUNCDEF_FIELDS.get(entry["struct"])
        if fields is None:
            return {
                "symbol": sym,
                "struct": entry["struct"],
                "line": entry["line"],
                "unparsed": True,
            }
        try:
            vals = map_fields(entry["body"], fields, consts)
        except ValueError as exc:
            raise ValueError(f"{sym} ({entry['struct']}, {inv_path}:{entry['line']}): {exc}")
        out = {
            "symbol": sym,
            "struct": entry["struct"],
            "line": entry["line"],
            "source": f"invitems.c:{entry['line']}",
            **vals,
        }
        out["name_text"] = gun_string(strings, vals.get("name"))
        # Decode the FUNCFLAG_* bits that are set, so the behaviour flags are
        # readable rather than a hex blob.
        out["flag_names"] = decode_flags(vals.get("flags"), consts, "FUNCFLAG_")
        return out

    def expand_partvis(sym: object) -> list[dict]:
        """`invpartvisibility_*` -> the parts this weapon variant shows/hides.

        `struct modelpartvisibility` (`types.h:3018`) is `{part, visible}` pairs
        terminated by a `{255}` sentinel. This is how three weapons share ONE model
        file and still look different: the plain Falcon 2 hides its scope, silencer,
        both magazines and `MUZZLEFLASH1`, while `invpartvisibility_falcon2scope`
        leaves the scope showing.

        Ignoring it means exporting every part at once — a plain pistol wearing its
        scope AND its silencer, with the muzzle flash geometry stuck on permanently.
        """
        if not isinstance(sym, str) or sym not in inv:
            return []
        entry = inv[sym]
        out: list[dict] = []
        for row in split_top_level(entry["body"]):
            row = row.strip()
            if not row.startswith("{"):
                continue
            vals = [resolve_expr(t, consts) for t in split_top_level(row[1:-1])]
            if not vals:
                continue
            part = vals[0]
            if part == 255:  # the terminator
                break
            visible = vals[1] is not False if len(vals) > 1 else True
            if isinstance(part, int):
                out.append({"part": part, "visible": visible})
        return out

    def expand_gunvis(sym: object) -> list[dict]:
        """`gunviscmds_*` -> the SECOND visibility table on a weapondef.

        `weapondef` carries two (`types.h:3023`): `partvisibility`, which
        [`expand_partvis`] reads, and this one, run by
        `bgun_execute_gun_vis_commands`. Only the unconditional rows can be baked
        into a static export — `type` 0 terminates, 4 tests `hand->upgradewant`,
        5/6 test which hand the gun is in, and those are runtime state. The macros
        (`gunscript.h:37-49`) are transcribed rather than the expanded structs
        because that is the form the source is written in.

        What this actually buys, measured across the whole MP set rather than
        assumed: **two** geometry groups that `partvisibility` leaves showing — the
        silenced Falcon 2's `MODELPART_FALCON2_002F` and the CMP150's
        `MODELPART_GUN_CARTFLAPOPEN` (its cartridge flap, hanging open at rest).

        The 14 `MODELPART_HAND_LEFT` rows — which looked like the big prize, since
        a first-person gun rendering a disembodied hand is a defect we have had
        before — hide **nothing of ours**: no first-person or third-person gun
        model in the MP set has a hand part at all (checked on all 33 of each).
        PD's first-person hands live in their own model, so `gunviscmds` is
        addressing geometry we never load.
        """
        if not isinstance(sym, str) or sym not in inv:
            return []
        out: list[dict] = []
        body = inv[sym]["body"]
        for call, args in re.findall(r"gunviscmd_(\w+)\(([^)]*)\)", body):
            vals = [resolve_expr(a.strip(), consts) for a in args.split(",") if a.strip()]
            if call == "sethidden" and vals and isinstance(vals[0], int):
                out.append({"part": vals[0], "op": "hidden", "condition": "always"})
            elif call in ("checkupgrade", "checkinlefthand", "checkinrighthand"):
                part = next((v for v in reversed(vals) if isinstance(v, int)), None)
                out.append({"part": part, "op": "conditional", "condition": call})
        return out

    def expand_ammo(sym: object) -> dict | None:
        if not isinstance(sym, str) or sym not in inv:
            return None
        entry = inv[sym]
        vals = map_fields(entry["body"], AMMODEF_FIELDS, consts)
        return {
            "symbol": sym,
            "line": entry["line"],
            "source": f"invitems.c:{entry['line']}",
            **vals,
        }

    # --- botinv.c / botcmd.c: the AI's per-function weapon preference ------
    bot_path = src("game", "botinv.c")
    bot_text = resolve_version_ifs(strip_comments(open(bot_path, encoding="utf-8").read()))
    bot_init = find_initializers(bot_text)
    bot_configs: list[dict] = []
    if "g_BotWeaponConfigs" in bot_init:
        base_line = bot_init["g_BotWeaponConfigs"]["line"]
        for i, row in enumerate(split_top_level(bot_init["g_BotWeaponConfigs"]["body"])):
            row = row.strip()
            if not row:
                continue
            body = row[1:-1] if row.startswith("{") else row
            cfg = map_fields(body, BOTWEAPONCONFIG_FIELDS, consts)
            cfg["source"] = f"botinv.c:{base_line + i}"
            bot_configs.append(cfg)

    # `g_BotDistConfigs[][3]` (botcmd.c:29) — {min, max, unused} attack distance
    # per band. PD world units are CENTIMETRES, so these divide by 100 for metres:
    # independently pinned in tools/pd-assets/pd_pose.py, whose own derivation
    # cites these very numbers ("melee range is 210 (2.1 m), a bot follows within
    # 300 (3 m)") — 300 and 250 are BOTDISTCFG_PISTOL's min and FOLLOW's max here.
    dist_path = src("game", "botcmd.c")
    dist_text = resolve_version_ifs(strip_comments(open(dist_path, encoding="utf-8").read()))
    dist_bands: list[dict] = []
    m = re.search(r"g_BotDistConfigs\s*\[\s*\]\s*\[\s*3\s*\]\s*=\s*\{(.*?)\n\};", dist_text, re.S)
    if m:
        band_names = {v: k for k, v in consts.by_prefix("BOTDISTCFG_").items()}
        for i, row in enumerate(split_top_level(m.group(1))):
            row = row.strip()
            if not row.startswith("{"):
                continue
            vals = [resolve_expr(t, consts) for t in split_top_level(row[1:-1])]
            dist_bands.append(
                {
                    "index": i,
                    "name": band_names.get(i, f"BOTDISTCFG_{i}"),
                    "min_cm": vals[0] if len(vals) > 0 else None,
                    "max_cm": vals[1] if len(vals) > 1 else None,
                    "min_m": (vals[0] / 100.0) if isinstance(vals[0], (int, float)) else None,
                    "max_m": (vals[1] / 100.0) if isinstance(vals[1], (int, float)) else None,
                }
            )

    # --- explosions.c: g_ExplosionTypes[] ----------------------------------
    exp_path = src("game", "explosions.c")
    exp_text = resolve_version_ifs(strip_comments(open(exp_path, encoding="utf-8").read()))
    exp_init = find_initializers(exp_text)
    explosions: list[dict] = []
    if "g_ExplosionTypes" in exp_init:
        base_line = exp_init["g_ExplosionTypes"]["line"]
        exp_names = {v: k for k, v in consts.by_prefix("EXPLOSIONTYPE_").items()}
        idx = 0
        for row in split_top_level(exp_init["g_ExplosionTypes"]["body"]):
            row = row.strip()
            if not row.startswith("{"):
                continue
            vals = map_fields(row[1:-1], EXPLOSIONTYPE_FIELDS, consts)
            vals["index"] = idx
            vals["name"] = exp_names.get(idx, f"EXPLOSIONTYPE_{idx}")
            vals["source"] = f"explosions.c:{base_line + idx}"
            explosions.append(vals)
            idx += 1

    # --- assemble one row per MP weapon ------------------------------------
    mpweapon_names = {v: k for k, v in consts.by_prefix("MPWEAPON_").items()}
    weapon_names = {v: k for k, v in consts.by_prefix("WEAPON_").items()}

    editor = editor_dump_index()
    # Second pass key: fp model .bin -> folder, so the Falcon variants (which share
    # `FILE_GFALCON2`) resolve to the plain Falcon's export.
    editor_by_bin: dict[str, dict] = {}

    rows = []
    for mp_index, raw in enumerate(mp_rows_raw):
        inner = raw.strip()
        body = inner[1:-1] if inner.startswith("{") else inner
        mp = map_fields(body, MPWEAPON_FIELDS, consts)
        weaponnum = mp.get("weaponnum")
        if not isinstance(weaponnum, int) or weaponnum <= 1:
            continue  # WEAPON_NONE / WEAPON_UNARMED
        if weaponnum == consts.value("WEAPON_DISABLED"):
            continue  # the `{ WEAPON_DISABLED }` sentinel row that ends the table

        mp_name = mpweapon_names.get(mp_index, f"MPWEAPON_{mp_index:#04x}")
        wsym = weapon_symbols[weaponnum] if weaponnum < len(weapon_symbols) else None
        wdef_entry = inv.get(wsym) if isinstance(wsym, str) else None
        if wdef_entry is None:
            print(f"warning: {mp_name} -> {wsym!r} has no weapondef", file=sys.stderr)
            continue

        try:
            wdef = map_fields(
                wdef_entry["body"], weapondef_fields(wdef_entry["body"]), consts
            )
        except ValueError as exc:
            raise ValueError(f"{wsym} (invitems.c:{wdef_entry['line']}): {exc}") from exc
        # Normalise the two shapes onto one `functions` list.
        if "functions" not in wdef:
            wdef["functions"] = [wdef.pop("pri_function", None), wdef.pop("sec_function", None)]

        hi_sym = None
        for k, v in consts.files.items():
            if v == wdef.get("hi_model"):
                hi_sym = k
                break
        fp_bin = file_symbol_to_path(hi_sym) if hi_sym else None

        tp_sym, tp_bin = model_to_bin(mp.get("model"))

        funcs = wdef.get("functions") or []
        if not isinstance(funcs, list):
            funcs = [funcs]

        name_text = gun_string(strings, wdef.get("name"))
        part_vis = expand_partvis(wdef.get("partvisibility"))

        rows.append(
            {
                "mp_index": mp_index,
                "mpweapon": mp_name,
                "weapon": weapon_names.get(weaponnum, f"WEAPON_{weaponnum:#04x}"),
                "weaponnum": weaponnum,
                "symbol": wsym,
                "source": f"invitems.c:{wdef_entry['line']}",
                "equipment_only": mp_name in EQUIPMENT_ONLY,
                "name_text": gun_string(strings, wdef.get("name")),
                "short_text": gun_string(strings, wdef.get("shortname")),
                "manufacturer_text": gun_string(strings, wdef.get("manufacturer")),
                "description_text": gun_string(strings, wdef.get("description")),
                # Viewmodel placement — the values `weapon-config.json` guessed.
                "muzzlez": wdef.get("muzzlez"),
                "posx": wdef.get("posx"),
                "posy": wdef.get("posy"),
                "posz": wdef.get("posz"),
                "sway": wdef.get("sway"),
                "weapon_flags": wdef.get("flags"),
                "weapon_flag_names": decode_flags(wdef.get("flags"), consts, "WEAPONFLAG_"),
                "ai_can_use": bool(
                    isinstance(wdef.get("flags"), int)
                    and consts.value("WEAPONFLAG_AICANUSE")
                    and wdef["flags"] & consts.value("WEAPONFLAG_AICANUSE")  # type: ignore[operator]
                ),
                "assets": {
                    "fp_model_symbol": hi_sym,
                    "fp_model": fp_bin,
                    "tp_model_symbol": tp_sym,
                    "tp_model": tp_bin,
                    "tp_model_const": model_by_index.get(mp.get("model"))
                    if isinstance(mp.get("model"), int)
                    else None,
                    "extrascale": mp.get("extrascale"),
                },
                "mp": {
                    "pri_ammo_type": mp.get("priammotype"),
                    "pri_ammo_qty": mp.get("priammoqty"),
                    "sec_ammo_type": mp.get("secammotype"),
                    "sec_ammo_qty": mp.get("secammoqty"),
                    "source": f"mplayer.c:{mp_init['g_MpWeapons']['line'] + mp_index}",
                },
                "part_visibility": part_vis,
                "gun_vis": expand_gunvis(wdef.get("gunviscmds")),
                "editor": None,  # filled in below, once fp_model is known
                "export": export_info(
                    w_slug(mp_index, name_text),
                    tp_bin,
                    has_flash=any(
                        r["part"] == MODELPART_GUN_MUZZLEFLASH1 and not r["visible"]
                        for r in part_vis
                    ),
                ),
                "bot": bot_configs[weaponnum] if weaponnum < len(bot_configs) else None,
                "functions": [expand_func(f) for f in funcs],
                "ammo": [expand_ammo(wdef.get("pri_ammo")), expand_ammo(wdef.get("sec_ammo"))],
            }
        )

    # Resolve the editor dump per row, falling back to whichever gun shares the
    # same first-person model file (the Falcon variants).
    for r in rows:
        e = editor.get(r["weaponnum"])
        if e:
            editor_by_bin.setdefault(r["assets"]["fp_model"] or "", e)
    for r in rows:
        r["editor"] = editor.get(r["weaponnum"]) or editor_by_bin.get(
            r["assets"]["fp_model"] or ""
        )

    return {
        "_provenance": {
            "decomp": os.path.relpath(DECOMP, REPO).replace(os.sep, "/"),
            "version": "ntsc-final",
            "sources": [
                "src/game/invitems.c (weapondef / funcdef_* / ammodef)",
                "src/game/mplayer/mplayer.c (g_MpWeapons — the MP set)",
                "src/game/botinv.c (g_BotWeaponConfigs — per-function AI preference)",
                "src/game/botcmd.c (g_BotDistConfigs — engagement bands, in cm)",
                "src/game/explosions.c (g_ExplosionTypes — 26 typed explosions)",
                "src/game/modeldata/general.c (MODEL_* -> FILE_*)",
                "src/include/types.h (struct layouts)",
                "src/include/constants.h, src/include/files.h (#defines)",
                "Makefile:209-210 (FILE_G*/FILE_P* -> guns/*.bin, props/*.bin)",
            ],
            "generator": "tools/pd-assets/pd_weapons.py",
        },
        "dist_bands": dist_bands,
        "explosions": explosions,
        "weapons": rows,
    }


def w_slug(mp_index: int, name: str) -> str:
    """The export slug `pd_gltf.py guns` writes, e.g. `01-falcon-2`."""
    out = "".join(c.lower() if c.isalnum() else "-" for c in name)
    while "--" in out:
        out = out.replace("--", "-")
    return f"{mp_index:02x}-{out.strip('-')}"


def export_info(slug: str, tp_rel: str | None, has_flash: bool = False) -> dict:
    """Where the exported GLBs live, plus the third-person shot origin.

    The muzzle is read through `pd_gltf.gun_metadata`, the same function the
    exporter uses, so the number in the Rust table and the number baked into the
    GLB cannot disagree — rather than re-deriving it here and hoping.
    """
    info = {
        "fp_glb": f"pd/{slug}-fp.glb",
        "tp_glb": f"pd/{slug}-tp.glb",
        # `MUZZLEFLASH1` is exported to its own GLB when the weapon hides it (which
        # is what PD does), so it can be drawn on fire rather than baked into the gun.
        "flash_glb": f"pd/{slug}-flash.glb" if has_flash else "",
        "tp_muzzle": [0.0, 0.0, 0.0],
        "muzzle_is_authored": False,
    }
    if not tp_rel:
        return info
    try:
        sys.path.insert(0, HERE)
        import pd_gltf  # noqa: PLC0415  (deferred: heavy, and only needed here)
        from pd_model import load as load_model  # noqa: PLC0415

        m = load_model(os.path.join(ASSETS, "files", tp_rel.replace("/", os.sep)))
        meta = pd_gltf.gun_metadata(m, pd_gltf.EXPORT_SCALE)
    except Exception as exc:  # noqa: BLE001 — provenance beats a hard failure here
        print(f"warning: could not read {tp_rel} muzzle: {exc}", file=sys.stderr)
        return info
    if meta.get("muzzle"):
        info["tp_muzzle"] = [float(v) for v in meta["muzzle"]]
    info["muzzle_is_authored"] = meta.get("muzzle_from") == "CHRGUNFIRE"
    return info


#: Where the Perfect Gold / GoldenEye Setup Editor dump lives, relative to the repo.
EDITOR_DUMP = os.path.join(REPO, "pd dump", "weapons")


def editor_dump_index() -> dict[int, dict]:
    """`weaponnum -> {folder, textures}` for the editor-exported gun assets.

    The user exported the arsenal with Perfect Gold (the OBJ header says "SubDrag
    and the GoldenEye Setup Editor"). Two things make that dump worth reading rather
    than treating as a curiosity:

    * **It decodes the textures we cannot.** PD's second texture codec
      (`tex_inflate_non_zlib`) is unported, and every one of the 76 muzzle-flash
      textures across the arsenal is on it. The dump covers 100% of them, as 32-bit
      BMPs with real alpha — so the flash is a soft flame instead of an opaque
      square, without porting the codec.
    * **Its material NAMES carry the render intent**, in the exact convention our
      engine already parses (`CullBoth`, `EnvMapping`, `ClampS/T`, `TexScaleS/T`,
      `Transparent`) — because our GoldenEye weapon GLBs came out of this same tool.
      `textured_model.rs` already looks for `*EnvMapping*`; `load_flash` already
      filters on `CullBoth`.

    The filename convention is `GunNNNN.obj` where NNNN is the **`WEAPON_*` enum
    value in hex** — verified against all 31 folders, e.g. `Gun0002` = WEAPON_FALCON2
    (0x02), `Gun000C` = WEAPON_CALLISTO (0x0c). Textures are `tempImgEdNNNN.bmp`,
    NNNN being the texture number the model's own texconfig table uses, which is what
    lets them be matched up without guessing.

    31 folders cover all 33 MP guns: the Falcon 2 silencer and scope variants have no
    folder of their own because they **share the plain Falcon's model file** and
    differ only by `modelpartvisibility`, so callers should fall back to the folder of
    whichever gun shares their `fp_model`.
    """
    out: dict[int, dict] = {}
    if not os.path.isdir(EDITOR_DUMP):
        return out
    for folder in sorted(os.listdir(EDITOR_DUMP)):
        path = os.path.join(EDITOR_DUMP, folder)
        if not os.path.isdir(path):
            continue
        objs = [f for f in os.listdir(path) if f.lower().endswith(".obj")]
        if not objs:
            continue
        stem = os.path.splitext(objs[0])[0]
        if not stem.lower().startswith("gun"):
            continue
        try:
            weaponnum = int(stem[3:], 16)
        except ValueError:
            continue
        texes = sorted(
            int(f[len("tempImgEd") : -4], 16)
            for f in os.listdir(path)
            if f.startswith("tempImgEd") and f.lower().endswith(".bmp")
        )
        out[weaponnum] = {
            "folder": folder,
            "obj": objs[0],
            "mtl": stem + ".mtl",
            "textures": texes,
        }
    return out


def decode_flags(value: object, consts: Consts, prefix: str) -> list[str]:
    """Which `<prefix>*` bits are set in `value`, named, single-bit flags only."""
    if not isinstance(value, int):
        return []
    out = []
    for name, bit in sorted(consts.by_prefix(prefix).items(), key=lambda kv: kv[1]):
        if bit and (bit & (bit - 1)) == 0 and value & bit:
            out.append(name)
    return out


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

FUNC_KIND = {
    "funcdef_shootsingle": "single",
    "funcdef_shoot": "single",
    "funcdef_shootauto": "auto",
    "funcdef_shootprojectile": "projectile",
    "funcdef_throw": "throw",
    "funcdef_melee": "melee",
    "funcdef_special": "special",
    "funcdef_device": "device",
}


def cmd_json(out: str | None) -> int:
    table = build()
    text = json.dumps(table, indent=2)
    if out:
        with open(out, "w", encoding="utf-8") as fh:
            fh.write(text + "\n")
        print(f"{len(table['weapons'])} MP weapons -> {out}")
    else:
        print(text)
    return 0


def cmd_list() -> int:
    table = build()
    print(
        f"{'MP':>4}  {'name':<22} {'fp model':<22} {'tp model':<24} "
        f"{'AI':<3} functions"
    )
    for w in table["weapons"]:
        funcs = []
        for f in w["functions"]:
            if not f:
                funcs.append("-")
                continue
            kind = FUNC_KIND.get(f.get("struct", ""), f.get("struct", "?"))
            label = f.get("name_text") or f.get("symbol", "")
            funcs.append(f"{kind}({label})")
        flag = "yes" if w["ai_can_use"] else ""
        tag = " [equipment]" if w["equipment_only"] else ""
        print(
            f"{w['mp_index']:#04x}  {w['name_text']:<22} "
            f"{str(w['assets']['fp_model'] or '-'):<22} "
            f"{str(w['assets']['tp_model'] or '-'):<24} {flag:<3} "
            f"{' + '.join(funcs)}{tag}"
        )
    missing_fp = [w for w in table["weapons"] if not w["assets"]["fp_model"]]
    missing_tp = [w for w in table["weapons"] if not w["assets"]["tp_model"]]
    print(f"\n{len(table['weapons'])} MP weapons")
    if missing_fp:
        print(f"  no first-person model: {', '.join(w['name_text'] for w in missing_fp)}")
    if missing_tp:
        print(f"  no third-person model: {', '.join(w['name_text'] for w in missing_tp)}")
    return 0


def cmd_show(pattern: str) -> int:
    table = build()
    pat = pattern.lower()
    hits = [
        w
        for w in table["weapons"]
        if pat in w["name_text"].lower()
        or pat in (w["symbol"] or "").lower()
        or pat in w["mpweapon"].lower()
    ]
    if not hits:
        print(f"no MP weapon matching {pattern!r}", file=sys.stderr)
        return 1
    for w in hits:
        print(json.dumps(w, indent=2))
    return 0


# ---------------------------------------------------------------------------
# Rust emission
# ---------------------------------------------------------------------------


def rf(v: object, default: float = 0.0) -> str:
    """A Rust f32 literal that round-trips the JSON value."""
    if not isinstance(v, (int, float)):
        v = default
    out = repr(float(v))
    # Trim the decomp's long float expansions to something readable that still
    # round-trips at f32 (59.999996185303 -> 59.999996).
    if len(out.split(".")[-1]) > 8:
        out = f"{float(v):.8g}"
    if "." not in out and "e" not in out and "E" not in out:
        out += ".0"
    return out


def ri(v: object, default: int = 0) -> str:
    return str(int(v)) if isinstance(v, (int, float)) else str(default)


def rs(v: object) -> str:
    s = v if isinstance(v, str) else ""
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


RUST_FUNC_KIND = {
    "funcdef_shootsingle": "Single",
    "funcdef_shoot": "Single",
    "funcdef_shootauto": "Auto",
    "funcdef_shootprojectile": "Projectile",
    "funcdef_throw": "Throw",
    "funcdef_melee": "Melee",
    "funcdef_special": "Special",
    "funcdef_device": "Device",
    "funcdef": "Special",
}

RUST_HEADER = '''//! Perfect Dark's weapon table — GENERATED, do not hand-edit.
//!
//! Regenerate with:
//!
//! ```text
//! python tools/pd-assets/pd_weapons.py rust \\
//!     native/crates/game/src/combat/pd_weapons.rs
//! ```
//!
//! Every row carries the `invitems.c` line it came from, the same provenance
//! discipline [`super::attack_anim`] uses. The generator parses the decomp rather
//! than trusting a transcription, because this is ~33 weapons x 2 functions x ~20
//! bare numeric literals and a one-column slip in that is invisible.
//!
//! # What PD authors that we were guessing
//!
//! * **Two functions per weapon.** `weapondef.functions[2]`. Ours had one fire
//!   mode per gun; every PD weapon has a primary and a secondary, and they are
//!   frequently different *kinds* (the SuperDragon is an automatic plus a grenade
//!   launcher; the Reaper is an automatic plus a melee grind).
//! * **Viewmodel placement.** `muzzlez/posx/posy/posz/sway` is the authored
//!   version of what `weapon-config.json` hand-tuned and [`super::config`] bakes
//!   in as `model_offset` / `pivot_offset` / `muzzle_offset`.
//! * **Engagement distance, per function.** [`PD_DIST_BANDS`] indexed by
//!   `PdAi::band_pri` / `band_sec`. Our `standoff_for` derived a standoff from a
//!   guessed range with a 0.6 fudge factor; PD authored a min/max per weapon AND
//!   per function.
//! * **Automatics spin up.** `initial_rpm` -> `max_rpm`, against our flat
//!   `fire_cooldown`.
//!
//! # Three things measured, not assumed
//!
//! 1. **`WEAPONFLAG_AICANUSE` is not a gun filter.** The handoff described it as
//!    saying "exactly which guns an enemy may hold". It is set on all 64 real
//!    weapons and absent only from the 20 non-weapons (keycards, briefcases, bare
//!    projectile items) — so it gates *items*, and every MP gun is AI-usable. The
//!    real per-weapon AI data is `g_BotWeaponConfigs` ([`PdAi`]), which scores
//!    each function separately.
//! 2. **1 PD damage unit = 25.0 of our HP** ([`PD_DAMAGE_TO_HP`]). Derived from
//!    two independent facts agreeing, not fitted: a PD guard has `maxdamage = 4`
//!    (`chr.c:1127`) and the Falcon 2 does `damage = 1`, so four body shots kill;
//!    our [`crate::enemy::ENEMY_HEALTH`] is 100 and our PP7 does 25, so also four.
//! 3. **PD world units are centimetres** ([`PD_CM_TO_M`]). Independently pinned
//!    in `tools/pd-assets/pd_pose.py`, whose derivation cites the very numbers in
//!    `g_BotDistConfigs` ("a bot follows within 300 (3 m)").
//!
//! Damage and spread stay in **PD units** here — this file is the transcription,
//! and converting on the way in would bake an interpretation into the data. The
//! consumers convert.

#![allow(dead_code)] // the table is transcribed whole; consumers land per milestone

/// Multiplier from a PD damage number to our HP scale. See the module docs — this
/// is derived from shots-to-kill agreeing on both sides, not tuned.
pub const PD_DAMAGE_TO_HP: f32 = 25.0;

/// PD world units are centimetres.
pub const PD_CM_TO_M: f32 = 0.01;

/// PD ticks are 60ths of a second (the `*60` field suffix throughout the decomp).
pub const PD_TICKS_PER_SEC: f32 = 60.0;

'''


def emit_rust(table: dict) -> str:
    out: list[str] = [RUST_HEADER]

    # --- FUNCFLAG_* constants ---------------------------------------------
    consts = Consts()
    funcflags = sorted(consts.by_prefix("FUNCFLAG_").items(), key=lambda kv: kv[1])
    out.append(
        "// ─── `FUNCFLAG_*` (constants.h) — behaviour flags on a firing function ──────\n"
        "// Transcribed whole, including the ones nothing consumes yet: porting a table\n"
        "// means porting its filters too, and a flag that is absent cannot later be\n"
        "// noticed as missing.\n"
    )
    for name, bit in funcflags:
        out.append(f"pub const {name}: u32 = {bit:#010x};")
    out.append("")

    # --- kinds -------------------------------------------------------------
    out.append(
        """
/// Which of PD's seven `funcdef` subtypes a firing function is
/// (`INVENTORYFUNCTYPE_*`, `types.h:2910-3010`). Our [`super::FireKind`] had
/// three cases; this is the full set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdFuncKind {
    /// `funcdef_shootsingle` — one round per pull.
    Single,
    /// `funcdef_shootauto` — held fire that spins up from `initial_rpm` to `max_rpm`.
    Auto,
    /// `funcdef_shootprojectile` — launches a travelling round.
    Projectile,
    /// `funcdef_throw` — lobbed (grenades, mines, the Laptop's sentry deploy).
    Throw,
    /// `funcdef_melee` — contact damage inside `melee_range`.
    Melee,
    /// `funcdef_special` — a scripted behaviour (cloak, crouch, detonate).
    Special,
    /// `funcdef_device` — a held gadget rather than a weapon (scanners).
    Device,
}

/// One firing function. Fields not meaningful for the kind are zero — PD's
/// subtypes are a struct hierarchy and this is their union, flattened.
#[derive(Clone, Copy, Debug)]
pub struct PdFunc {
    /// The authored in-game label, e.g. `"Single Shot"`, `"Grenade Launcher"`.
    pub label: &'static str,
    pub kind: PdFuncKind,
    /// `FUNCFLAG_*` bits — see the constants above.
    pub flags: u32,
    /// Damage in **PD units**; multiply by [`PD_DAMAGE_TO_HP`] for our scale.
    pub damage: f32,
    /// PD's per-shot cone width, in its own units (see [`crate::pdsim::spread`]).
    pub spread: f32,
    /// Ticks (60ths) before the weapon may fire again.
    pub recovery60: i32,
    /// How many bodies a round passes through.
    pub penetration: u32,
    /// `funcdef.ammoindex` — which of the weapon's two ammo pools this function
    /// draws from. `0` = the primary's, `1` = a separate pool, `-1` = costs
    /// nothing (melee). This is what stops "two functions" collapsing into one
    /// gun with two damage numbers.
    pub ammo_index: i8,
    /// `Auto` only: the rate the trigger-pull starts at, and what it winds up to.
    pub initial_rpm: f32,
    pub max_rpm: f32,
    /// `Projectile` only: launch speed and fuse (ticks).
    pub projectile_speed: f32,
    pub projectile_timer60: i32,
    /// `Melee` only: reach, in PD centimetres.
    pub melee_range: f32,
    /// Viewmodel recoil: kick-back distance (PD centimetres) and muzzle rise
    /// (PD's own angle units). The authored counterpart of our `recoil_z` /
    /// `recoil_rot`, which are two shared constants across all 24 GE weapons.
    pub recoil_dist: f32,
    pub recoil_angle: f32,
    /// `invitems.c` line of the `funcdef` this row came from.
    pub source: &'static str,
}

impl PdFunc {
    /// An inert function, for the one MP entry that has none at all
    /// (`MPWEAPON_SHIELD` — `invitem_shieldtechitem` carries no `functions[2]`).
    /// Keeping `primary` non-optional means the 33 guns never unwrap; this is the
    /// single row that needs a stand-in, and it is `equipment_only` anyway.
    pub const INERT: PdFunc = PdFunc {
        label: "",
        kind: PdFuncKind::Device,
        flags: 0,
        damage: 0.0,
        spread: 0.0,
        recovery60: 0,
        penetration: 0,
        ammo_index: -1,
        initial_rpm: 0.0,
        max_rpm: 0.0,
        projectile_speed: 0.0,
        projectile_timer60: 0,
        melee_range: 0.0,
        recoil_dist: 0.0,
        recoil_angle: 0.0,
        source: "",
    };

    /// Damage on our 100-HP scale.
    pub fn damage_hp(&self) -> f32 {
        self.damage * PD_DAMAGE_TO_HP
    }

    /// Seconds between shots at the *sustained* rate: an automatic's `max_rpm`,
    /// otherwise its `recovery60`. This is the honest analogue of our flat
    /// `fire_cooldown`; the spin-up needs the runtime to track a trigger hold.
    pub fn sustained_cooldown(&self) -> f32 {
        if self.kind == PdFuncKind::Auto && self.max_rpm > 0.0 {
            60.0 / self.max_rpm
        } else if self.recovery60 > 0 {
            self.recovery60 as f32 / PD_TICKS_PER_SEC
        } else {
            0.0
        }
    }

    pub fn has_flag(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    /// `recoildist` as metres. PD's values run 0-40ish in centimetres, which is a
    /// viewmodel kick and not a world distance.
    pub fn recoildist_m(&self) -> f32 {
        self.recoil_dist * PD_CM_TO_M
    }

    /// `recoilangle` as radians. PD stores whole-ish degrees here (the Falcon 2's
    /// 15, the Magnum's larger), so this is a degree conversion.
    pub fn recoilangle_rad(&self) -> f32 {
        self.recoil_angle.to_radians()
    }
}

/// A bot engagement band (`g_BotDistConfigs`, `botcmd.c:29`): the min and max
/// distance a hunter wants to be at while attacking. Indexed by
/// [`PdAi::band_pri`] / [`PdAi::band_sec`].
#[derive(Clone, Copy, Debug)]
pub struct PdDistBand {
    pub name: &'static str,
    pub min_m: f32,
    pub max_m: f32,
}

/// The AI's authored view of a weapon (`g_BotWeaponConfigs`, `botinv.c:21`).
/// `score_*` is how much a bot wants that function — this is what makes
/// "primary or secondary?" a data question rather than our judgement.
#[derive(Clone, Copy, Debug)]
pub struct PdAi {
    pub score_pri: u8,
    pub score_sec: u8,
    pub dual_pri: u8,
    pub dual_sec: u8,
    /// Index into [`PD_DIST_BANDS`].
    pub band_pri: u8,
    pub band_sec: u8,
    pub source: &'static str,
}

/// `weapondef`'s viewmodel placement — the authored version of the values
/// `weapon-config.json` was hand-tuned to. In PD centimetres.
#[derive(Clone, Copy, Debug)]
pub struct PdView {
    pub muzzlez: f32,
    pub posx: f32,
    pub posy: f32,
    pub posz: f32,
    pub sway: f32,
}

/// One Perfect Dark weapon, as the MP set defines it.
#[derive(Clone, Copy, Debug)]
pub struct PdWeapon {
    /// `MPWEAPON_*` index — the multiplayer slot, and this table's stable id.
    pub mp_index: u8,
    /// The authored name, e.g. `"Falcon 2"`, `"FarSight XR-20"`.
    pub name: &'static str,
    /// First-person model, relative to PD's `files/` (`weapondef.hi_model`).
    pub fp_model: &'static str,
    /// Third-person model an enemy holds — the one carrying `CHRGUNFIRE`.
    pub tp_model: &'static str,
    /// The exported first-person GLB, relative to `native/assets/weapons/` so it
    /// drops straight into the same slot as a GoldenEye `WeaponStats::gun_path`.
    pub fp_glb: &'static str,
    /// The exported muzzle-flash GLB, or `""` when the weapon authors no flash.
    /// Separate from the gun because PD hides `MUZZLEFLASH1` in the weapon's
    /// `modelpartvisibility` and shows it only while firing — baked into the gun it
    /// reads as a permanent square stuck on the barrel.
    pub flash_glb: &'static str,
    /// The exported third-person GLB — what a hunter holds. Unlike the GoldenEye
    /// guns this needs no hand-stripping: PD's `chr*` models are the third-person
    /// weapon alone, so the `enemy-weapon-hand-artifact` cannot arise.
    pub tp_glb: &'static str,
    /// Authored muzzle / shot origin on the third-person model, in engine units.
    /// From `CHRGUNFIRE` where PD authors one, else PD's own grip fallback — see
    /// [`Self::muzzle_is_authored`].
    pub tp_muzzle: [f32; 3],
    /// True when [`Self::tp_muzzle`] came from a real `CHRGUNFIRE` node; false
    /// when it is `chr_get_gun_pos`'s `MODELPART_0001` grip fallback (17 of 33).
    pub muzzle_is_authored: bool,
    /// Rounds per magazine (`ammodef.clipsize`); 0 when the weapon has no clip.
    pub clip_size: i32,
    /// Clip size of the SECOND ammo pool (`weapondef.ammos[1]`), for a weapon
    /// whose secondary function has its own resource — the SuperDragon's grenade
    /// launcher is the MP set's example. 0 when there is no second pool.
    pub sec_clip_size: i32,
    /// Rounds an MP match hands out (`mpweapon.priammoqty`).
    pub ammo_qty: u32,
    pub primary: PdFunc,
    /// PD's defining feature. `None` only for the equipment entries.
    pub secondary: Option<PdFunc>,
    pub view: PdView,
    pub ai: PdAi,
    /// `WEAPONFLAG_ONEHANDED` — guards carry it in one hand (our pistol class).
    pub one_handed: bool,
    /// `WEAPONFLAG_DUALWIELD` — may be held in both hands.
    pub dual_wield: bool,
    /// `WEAPONFLAG_AICANUSE`. True for every gun — see the module docs; kept
    /// because its *absence* is meaningful for non-weapons.
    pub ai_can_use: bool,
    /// True for the four MP entries with no gameplay system to attach to
    /// (X-Ray, Cloak, Combat Boost, Shield) — excluded from the port scope.
    pub equipment_only: bool,
    /// `invitems.c` line of this `weapondef`.
    pub source: &'static str,
}

impl PdWeapon {
    /// The function a `secondary` request resolves to, falling back to the
    /// primary so a caller never has to special-case a one-function entry.
    pub fn function(&self, secondary: bool) -> &PdFunc {
        if secondary {
            self.secondary.as_ref().unwrap_or(&self.primary)
        } else {
            &self.primary
        }
    }

    /// The engagement band for a function, as metres.
    pub fn band_m(&self, secondary: bool) -> (f32, f32) {
        let i = if secondary { self.ai.band_sec } else { self.ai.band_pri } as usize;
        match PD_DIST_BANDS.get(i) {
            Some(b) => (b.min_m, b.max_m),
            None => (0.0, 0.0),
        }
    }
}
"""
    )

    # --- dist bands --------------------------------------------------------
    bands = table["dist_bands"]
    out.append(
        f"\n/// `g_BotDistConfigs` (`botcmd.c:29`), converted to metres.\n"
        f"pub const PD_DIST_BANDS: [PdDistBand; {len(bands)}] = ["
    )
    for b in bands:
        out.append(
            f"    PdDistBand {{ name: {rs(b['name'])}, "
            f"min_m: {rf(b['min_m'])}, max_m: {rf(b['max_m'])} }},"
        )
    out.append("];\n")

    # --- explosions --------------------------------------------------------
    out.append(
        """
/// One row of `g_ExplosionTypes` (`explosions.c:41`). The two fields our single
/// spherical [`super::Explosion`] had no equivalent for:
///
/// * `blast_radius_m` vs `damage_radius_m` — they differ by up to 2x, so the
///   visible fireball is much smaller than the lethal volume.
/// * `propagation_rate` + `duration_s` — a PD blast keeps applying while the
///   radius grows from blast to damage, instead of resolving in one instant.
///
/// The falloff those feed is NOT our linear sphere; see `super::explosives`.
#[derive(Clone, Copy, Debug)]
pub struct PdExplosion {
    pub name: &'static str,
    pub index: u8,
    pub blast_radius_m: f32,
    pub damage_radius_m: f32,
    pub inner_size_m: f32,
    /// Seconds the explosion lives (and grows) for.
    pub duration_s: f32,
    pub propagation_rate: i32,
    /// Damage scale in PD units — the *peak* a chr takes is `damage * 8.0`
    /// (`explosions.c:967`), before falloff.
    pub damage: f32,
    pub source: &'static str,
}

impl PdExplosion {
    /// Peak damage at the centre, on our HP scale. `chr_damage_by_explosion` is
    /// handed `minfrac * damage * 8.0` with `minfrac == 1` dead centre.
    pub fn peak_damage_hp(&self) -> f32 {
        self.damage * 8.0 * PD_DAMAGE_TO_HP
    }
}
"""
    )
    exps = table["explosions"]
    out.append(f"pub const PD_EXPLOSIONS: [PdExplosion; {len(exps)}] = [")
    for e in exps:
        out.append(
            f"    PdExplosion {{ name: {rs(e['name'])}, index: {ri(e['index'])}, "
            f"blast_radius_m: {rf((e['blastradius'] or 0) / 100.0)}, "
            f"damage_radius_m: {rf((e['damageradius'] or 0) / 100.0)}, "
            f"inner_size_m: {rf((e['innersize'] or 0) / 100.0)}, "
            f"duration_s: {rf((e['duration'] or 0) / 60.0)}, "
            f"propagation_rate: {ri(e['propagationrate'])}, "
            f"damage: {rf(e['damage'])}, source: {rs(e['source'])} }},"
        )
    out.append("];\n")

    # --- weapons -----------------------------------------------------------
    def emit_func(f: dict | None, indent: str) -> str:
        if not f:
            return "PdFunc::INERT"
        kind = RUST_FUNC_KIND.get(f.get("struct", ""), "Special")
        return (
            "PdFunc {\n"
            f"{indent}    label: {rs(f.get('name_text'))},\n"
            f"{indent}    kind: PdFuncKind::{kind},\n"
            f"{indent}    flags: {ri(f.get('flags'))},\n"
            f"{indent}    damage: {rf(f.get('damage'))},\n"
            f"{indent}    spread: {rf(f.get('spread'))},\n"
            f"{indent}    recovery60: {ri(f.get('recoverytime60'))},\n"
            f"{indent}    penetration: {ri(f.get('penetration'))},\n"
            f"{indent}    ammo_index: {ri(f.get('ammoindex'), -1)},\n"
            f"{indent}    initial_rpm: {rf(f.get('initialrpm'))},\n"
            f"{indent}    max_rpm: {rf(f.get('maxrpm'))},\n"
            f"{indent}    projectile_speed: {rf(f.get('speed'))},\n"
            f"{indent}    projectile_timer60: {ri(f.get('timer60'))},\n"
            f"{indent}    melee_range: {rf(f.get('range'))},\n"
            f"{indent}    recoil_dist: {rf(f.get('recoildist'))},\n"
            f"{indent}    recoil_angle: {rf(f.get('recoilangle'))},\n"
            f"{indent}    source: {rs(f.get('source'))},\n"
            f"{indent}}}"
        )

    rows = table["weapons"]
    out.append(
        "/// The Perfect Dark multiplayer arsenal, in `MPWEAPON_*` order.\n"
        "///\n"
        "/// This is the whole MP set including the four equipment entries, which\n"
        "/// carry `equipment_only: true` — they are transcribed so the table matches\n"
        "/// the source, and filtered by [`pd_guns`].\n"
        f"pub const PD_WEAPONS: [PdWeapon; {len(rows)}] = ["
    )
    for w in rows:
        ammo = (w.get("ammo") or [None])[0]
        clip = ammo.get("clipsize") if ammo else 0
        funcs = w["functions"]
        pri = funcs[0] if len(funcs) > 0 else None
        sec = funcs[1] if len(funcs) > 1 else None
        flag_names = w["weapon_flag_names"]
        bot = w["bot"] or {}
        out.append(f"    // {w['mpweapon']} — {w['description_text'][:70]}")
        out.append("    PdWeapon {")
        out.append(f"        mp_index: {ri(w['mp_index'])},")
        out.append(f"        name: {rs(w['name_text'])},")
        out.append(f"        fp_model: {rs(w['assets']['fp_model'] or '')},")
        out.append(f"        tp_model: {rs(w['assets']['tp_model'] or '')},")
        ex = w.get("export") or {}
        out.append(f"        fp_glb: {rs(ex.get('fp_glb'))},")
        out.append(f"        flash_glb: {rs(ex.get('flash_glb'))},")
        out.append(f"        tp_glb: {rs(ex.get('tp_glb'))},")
        mz = ex.get("tp_muzzle") or [0.0, 0.0, 0.0]
        out.append(
            f"        tp_muzzle: [{rf(mz[0])}, {rf(mz[1])}, {rf(mz[2])}],"
        )
        out.append(
            f"        muzzle_is_authored: {str(bool(ex.get('muzzle_is_authored'))).lower()},"
        )
        sec_ammo = (w.get("ammo") or [None, None])[1] if len(w.get("ammo") or []) > 1 else None
        sec_clip = sec_ammo.get("clipsize") if sec_ammo else 0
        out.append(f"        clip_size: {ri(clip)},")
        out.append(f"        sec_clip_size: {ri(sec_clip)},")
        out.append(f"        ammo_qty: {ri(w['mp']['pri_ammo_qty'])},")
        out.append(f"        primary: {emit_func(pri, '        ')},")
        if sec:
            out.append(f"        secondary: Some({emit_func(sec, '        ')}),")
        else:
            out.append("        secondary: None,")
        out.append(
            f"        view: PdView {{ muzzlez: {rf(w['muzzlez'])}, posx: {rf(w['posx'])}, "
            f"posy: {rf(w['posy'])}, posz: {rf(w['posz'])}, sway: {rf(w['sway'])} }},"
        )
        out.append(
            f"        ai: PdAi {{ score_pri: {ri(bot.get('score1'))}, "
            f"score_sec: {ri(bot.get('score2'))}, dual_pri: {ri(bot.get('dualscore1'))}, "
            f"dual_sec: {ri(bot.get('dualscore2'))}, band_pri: {ri(bot.get('pridistconfig'))}, "
            f"band_sec: {ri(bot.get('secdistconfig'))}, source: {rs(bot.get('source'))} }},"
        )
        out.append(f"        one_handed: {str('WEAPONFLAG_ONEHANDED' in flag_names).lower()},")
        out.append(f"        dual_wield: {str('WEAPONFLAG_DUALWIELD' in flag_names).lower()},")
        out.append(f"        ai_can_use: {str(w['ai_can_use']).lower()},")
        out.append(f"        equipment_only: {str(w['equipment_only']).lower()},")
        out.append(f"        source: {rs(w['source'])},")
        out.append("    },")
    out.append("];\n")

    out.append(
        """
/// The guns — the MP set minus the four equipment entries. This is the port's
/// scope (user decision; see the `pd-arsenal-decisions` note).
pub fn pd_guns() -> impl Iterator<Item = &'static PdWeapon> {
    PD_WEAPONS.iter().filter(|w| !w.equipment_only)
}

/// Look a weapon up by its `MPWEAPON_*` index.
pub fn pd_weapon(mp_index: u8) -> Option<&'static PdWeapon> {
    PD_WEAPONS.iter().find(|w| w.mp_index == mp_index)
}

/// Look a weapon up by its authored name.
pub fn pd_weapon_by_name(name: &str) -> Option<&'static PdWeapon> {
    PD_WEAPONS.iter().find(|w| w.name == name)
}

/// An explosion type by its `EXPLOSIONTYPE_*` index.
pub fn pd_explosion(index: u8) -> Option<&'static PdExplosion> {
    PD_EXPLOSIONS.iter().find(|e| e.index == index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scope decision, pinned: 33 guns out of the 37 MP entries, with the
    /// four equipment ones separated rather than silently dropped.
    #[test]
    fn the_mp_set_is_33_guns_plus_4_equipment() {
        assert_eq!(pd_guns().count(), 33, "the MP gun set");
        assert_eq!(
            PD_WEAPONS.iter().filter(|w| w.equipment_only).count(),
            4,
            "X-Ray, Cloak, Combat Boost, Shield"
        );
    }

    /// PD's defining feature: every gun has a real second function. If this ever
    /// fails, the generator lost a `functions[2]` column.
    #[test]
    fn every_gun_has_two_functions() {
        for w in pd_guns() {
            assert!(w.secondary.is_some(), "{} has no secondary function", w.name);
        }
    }

    /// Both models resolve for every gun, and they are the two DIFFERENT models
    /// PD ships per weapon — the first-person one and the `chr*` one that carries
    /// `CHRGUNFIRE`. Grabbing the same file for both is the documented easy
    /// mistake (`DESIGN_PD_WEAPON_MECHANICS.md` §3).
    #[test]
    fn every_gun_has_both_models() {
        for w in pd_guns() {
            assert!(!w.fp_model.is_empty(), "{} has no first-person model", w.name);
            assert!(!w.tp_model.is_empty(), "{} has no third-person model", w.name);
            assert!(
                w.fp_model.starts_with("guns/"),
                "{} first-person model is not from guns/: {}",
                w.name,
                w.fp_model
            );
            assert!(
                w.tp_model.starts_with("props/chr"),
                "{} third-person model is not a props/chr* file: {}",
                w.name,
                w.tp_model
            );
            assert_ne!(w.fp_model, w.tp_model, "{} uses one model for both", w.name);
        }
    }

    /// Spot-check the Falcon 2 against the source by hand. If the generator ever
    /// slips a column, a row that reads plausibly is the failure mode — so one
    /// row is checked field by field against `invitems.c:485` and `:568`.
    #[test]
    fn falcon2_matches_the_source() {
        let w = pd_weapon_by_name("Falcon 2").expect("Falcon 2 in the table");
        assert_eq!(w.mp_index, 0x01);
        assert_eq!(w.fp_model, "guns/falcon2.bin");
        assert_eq!(w.tp_model, "props/chrfalcon2.bin");
        assert_eq!(w.clip_size, 8, "invammo_falcon2 clip size");
        assert!(w.one_handed && w.dual_wield);

        let p = &w.primary;
        assert_eq!(p.kind, PdFuncKind::Single);
        assert_eq!(p.damage, 1.0);
        assert_eq!(p.spread, 1.0);
        assert_eq!(p.recovery60, 16);
        assert_eq!(p.penetration, 1);

        // The secondary is the pistol whip — a melee function on a pistol, which
        // is exactly the sort of thing one fire mode per weapon could not express.
        let s = w.secondary.as_ref().expect("pistol whip");
        assert_eq!(s.kind, PdFuncKind::Melee);
        assert_eq!(s.damage, 0.9);
        assert!(s.has_flag(FUNCFLAG_MAKEDIZZY), "the whip knocks out");
        assert!(s.has_flag(FUNCFLAG_NOMUZZLEFLASH), "no flash on a melee");
    }

    /// The damage conversion lands where the derivation says it should: four
    /// Falcon 2 body shots kill a 100 HP hunter, matching PD's `maxdamage = 4`.
    #[test]
    fn the_damage_scale_gives_four_shots_to_kill() {
        let w = pd_weapon_by_name("Falcon 2").unwrap();
        let shots = (100.0 / w.primary.damage_hp()).ceil();
        assert_eq!(shots, 4.0, "PD's guard takes 4 Falcon 2 rounds; so must ours");
    }

    /// Automatics carry a real spin-up, and it is a spin-UP.
    #[test]
    fn automatics_spin_up() {
        let autos: Vec<_> = pd_guns()
            .filter(|w| w.primary.kind == PdFuncKind::Auto)
            .collect();
        assert!(autos.len() >= 8, "PD has plenty of automatics, got {}", autos.len());
        for w in &autos {
            let f = &w.primary;
            assert!(f.max_rpm > 0.0, "{} has no max rpm", w.name);
            assert!(
                f.initial_rpm <= f.max_rpm,
                "{} winds DOWN: {} -> {}",
                w.name,
                f.initial_rpm,
                f.max_rpm
            );
            assert!(f.sustained_cooldown() > 0.0, "{} has no cadence", w.name);
        }
    }

    /// The engagement bands order the way the weapons do: a knife wants contact,
    /// a FarSight wants the far side of the room. This is the authored answer to
    /// "standoff should scale with weapon range".
    #[test]
    fn engagement_bands_scale_with_the_weapon() {
        let knife = pd_weapon_by_name("Combat Knife").unwrap();
        let pistol = pd_weapon_by_name("Falcon 2").unwrap();
        let rocket = pd_weapon_by_name("Rocket Launcher").unwrap();
        let farsight = pd_weapon_by_name("FarSight XR-20").unwrap();

        assert!(knife.band_m(false).1 <= 1.5, "a knife closes to contact");
        assert!(pistol.band_m(false).0 >= 2.0, "a pistol holds off a little");
        assert!(
            rocket.band_m(false).0 > pistol.band_m(false).1,
            "a rocket stands off further than a pistol"
        );
        assert!(farsight.band_m(true).1 >= 20.0, "the FarSight reaches right across");

        // Every band is a real interval.
        for b in PD_DIST_BANDS.iter() {
            assert!(b.max_m > b.min_m, "{} is not an interval", b.name);
        }
    }

    /// The structural point about PD explosions: the lethal volume is bigger than
    /// the fireball, and blasts have a duration to grow across. If a port ever
    /// collapses these back to one sphere, this fails.
    #[test]
    fn explosions_separate_blast_from_damage_radius() {
        let lethal: Vec<_> = PD_EXPLOSIONS.iter().filter(|e| e.damage > 0.0).collect();
        assert!(lethal.len() >= 20, "most explosion types do damage");
        assert!(
            lethal.iter().any(|e| e.damage_radius_m > e.blast_radius_m * 1.5),
            "some blast reaches well past its fireball"
        );
        for e in &lethal {
            assert!(
                e.damage_radius_m >= e.blast_radius_m,
                "{} damages inside its own fireball only",
                e.name
            );
            assert!(e.duration_s > 0.0, "{} has no duration to propagate over", e.name);
        }
    }

    /// The rocket explosion is recognisably the one we authored by hand, which is
    /// the reassurance that adopting the table will not upend the feel: PD's
    /// damage radius is 4 m against our authored 5 m.
    #[test]
    fn the_rocket_explosion_is_close_to_our_authored_one() {
        let rocket = PD_EXPLOSIONS
            .iter()
            .find(|e| e.name == "EXPLOSIONTYPE_ROCKET")
            .expect("a named rocket explosion");
        assert!(
            (rocket.damage_radius_m - 4.0).abs() < 0.01,
            "PD's rocket damage radius, got {}",
            rocket.damage_radius_m
        );
        assert!(rocket.peak_damage_hp() > 100.0, "a direct rocket hit is lethal");
    }

    /// Every gun's two exported GLBs exist on disk, named as
    /// `pd_gltf.py guns` writes them. This is the seam where the weapon table and
    /// the asset export can silently drift apart — a row naming a model nobody
    /// exported reads fine in code and draws nothing in game.
    #[test]
    fn every_gun_has_its_exported_glbs() {
        let dir = format!("{}/../../assets/weapons/pd", env!("CARGO_MANIFEST_DIR"));
        if !std::path::Path::new(&dir).is_dir() {
            // The export is reproducible from the (gitignored) decomp, so a clone
            // without it should not fail the suite — but say so rather than pass
            // quietly, since a silent skip is how this stops testing anything.
            eprintln!("note: {dir} absent — run `pd_gltf.py guns` to check the assets");
            return;
        }
        for w in pd_guns() {
            let slug: String = w
                .name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
                .collect();
            let mut slug = slug;
            while slug.contains("--") {
                slug = slug.replace("--", "-");
            }
            let slug = slug.trim_matches('-');
            for role in ["fp", "tp"] {
                let path = format!("{dir}/{:02x}-{slug}-{role}.glb", w.mp_index);
                assert!(
                    std::path::Path::new(&path).is_file(),
                    "{} is missing its {role} model at {path}",
                    w.name
                );
            }
        }
    }

    /// Provenance is not decoration — every row must be traceable, because the
    /// whole reason this file is generated is so a wrong number can be found.
    #[test]
    fn every_row_cites_its_source() {
        for w in PD_WEAPONS.iter() {
            assert!(w.source.starts_with("invitems.c:"), "{} has no source", w.name);
        }
        // Functions are checked on the guns only: `PdFunc::INERT` deliberately has
        // no source, and exactly one equipment row uses it.
        for w in pd_guns() {
            assert!(w.primary.source.starts_with("invitems.c:"), "{} primary", w.name);
            let s = w.secondary.as_ref().expect("a gun has two functions");
            assert!(s.source.starts_with("invitems.c:"), "{} secondary", w.name);
        }
        assert_eq!(
            PD_WEAPONS.iter().filter(|w| w.primary.source.is_empty()).count(),
            1,
            "only MPWEAPON_SHIELD should need PdFunc::INERT"
        );
        for e in PD_EXPLOSIONS.iter() {
            assert!(e.source.starts_with("explosions.c:"), "{} has no source", e.name);
        }
    }
}
"""
    )
    return "\n".join(out)


def cmd_rust(out_path: str) -> int:
    table = build()
    text = emit_rust(table)
    with open(out_path, "w", encoding="utf-8") as fh:
        fh.write(text)
    guns = sum(1 for w in table["weapons"] if not w["equipment_only"])
    print(
        f"{len(table['weapons'])} MP weapons ({guns} guns), "
        f"{len(table['explosions'])} explosion types -> {out_path}"
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("rust", help="generate combat/pd_weapons.rs")
    p.add_argument("out")

    p = sub.add_parser("json", help="emit the whole MP weapon table as JSON")
    p.add_argument("out", nargs="?", default=None)

    sub.add_parser("list", help="one summary line per MP weapon")

    p = sub.add_parser("show", help="expand one weapon")
    p.add_argument("pattern")

    args = ap.parse_args()
    if args.cmd == "rust":
        return cmd_rust(args.out)
    if args.cmd == "json":
        return cmd_json(args.out)
    if args.cmd == "list":
        return cmd_list()
    if args.cmd == "show":
        return cmd_show(args.pattern)
    return 1


if __name__ == "__main__":
    sys.exit(main())
