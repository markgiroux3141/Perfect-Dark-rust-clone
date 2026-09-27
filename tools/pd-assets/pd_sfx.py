#!/usr/bin/env python3
"""Perfect Dark sound-effect (sfx.ctl + sfx.tbl) extractor -> 16-bit mono WAV.

Parses the N64 sound bank exactly the way the game loads it, decodes the VADPCM
sample data, resolves PD's `SFXNUM_*` / `SFXMAP_*` ids to bank sounds, and writes
WAVs at each sound's *natural* playback rate plus a JSON manifest. Stdlib only.
All citations are to `reference/pd-decomp/` unless prefixed `pcport:`
(`reference/pd-pcport/`).

# Bank layout (sfx.ctl)

Big-endian N64 `ALBankFile`. Every pointer field is an offset from the start of
the file and must be relocated like `alBnkfNew` does; PD does it by hand: ctl
offsets get `_sfxctlSegmentRomStart` added (lib/snd.c:972, :1017..:1238) and a
wavetable's `base` gets `_sfxtblSegmentRomStart` added, i.e. `base` is an offset
into sfx.tbl (lib/snd.c:1247). Structs (include/PR/libaudio.h):

    ALBankFile   :265  s16 revision (0x4231 'B1', :167); s16 bankCount; u32 bankArray[]
    ALBank       :256  s16 instCount; u8 flags; u8 pad; s32 sampleRate; u32 percussion; u32 instArray[]
    ALInstrument :238  12 x u8 (volume..vibDelay); s16 bendRange; s16 soundCount; u32 soundArray[]
    ALSound      :229  u32 envelope; u32 keyMap; u32 wavetable; u8 samplePan; u8 sampleVolume; u8 flags
    ALEnvelope   :192  s32 attackTime, decayTime, releaseTime (microseconds); u8 attackVolume, decayVolume
    ALKeyMap     :200  u8 velocityMin, velocityMax, keyMin, keyMax, keyBase; s8 detune
    ALWaveTable  :217  u32 base; s32 len; u8 type; u8 flags; (pad 2); union { adpcm: u32 loop, u32 book | raw: u32 loop }
    ALADPCMBook  :173  s32 order; s32 npredictors; s16 book[order*npredictors*8]
    ALADPCMloop  :179  u32 start, end, count; s16 state[16]      (ADPCM_STATE, include/PR/abi.h:245)
    ALRawLoop    :186  u32 start, end, count
    type         :170  AL_ADPCM_WAVE = 0, AL_RAW16_WAVE = 1

sfx.ctl holds one bank with one instrument (`snd_load_sfx_ctl`, lib/snd.c:925:
"the first (and only) bank ... the first (and only) instrument"). That
instrument's `soundArray` IS the game's sound table: `g_ALSoundRomOffsets` is the
instrument pointer advanced by 16 bytes (lib/snd.c:968), and sound number N is
`soundArray[N - 1]` (lib/snd.c:1352). Number 0 is "no sound"; `g_NumSounds =
soundCount + 1` (lib/snd.c:959) and `snd_start` rejects ids >= g_NumSounds
(lib/snd.c:2128). NTSC-final: soundCount = 1545, so numbers 0x001..0x609.

PD repurposes ALKeyMap fields (lib/naudio/n_sndplayer.c:11-17):

    KEYMAP_DELAY(m)    = velocityMax * 33333 us         start delay of this sound
    KEYMAP_FLAGS(m)    = keyMax & 0xf0                  SNDSTATEFLAG_* (constants.h:3831)
    KEYMAP_SOUNDNUM(m) = velocityMin + (keyMin & 0xc0)*4 next sound in a chain (0 = none)
    KEYMAP_VOLINDEX(m) = keyMin & 0x1f                  volume-table slot (all slots equal,
                                                         lib/snd.c:907 sets every one)

`sndp_play_sound` (n_sndplayer.c:718) walks that chain: playing N also starts
KEYMAP_SOUNDNUM(N) (and so on), each at its own KEYMAP_DELAY after the trigger
(n_sndplayer.c:763-779). The manifest records chains; the WAVs do not mix them.

# VADPCM decode

`len` is truncated to whole 9-byte frames (lib/naudio/n_load.c:203); each frame
is 16 samples (ADPCMFSIZE, include/PR/abi.h:244). Frame = 1 header byte
(`shift = hi nibble`, `predictor = lo nibble`) + 16 signed 4-bit residuals. Per
8-sample half-frame, with tbl = book[predictor] as [order=2][8] and prev1/prev2 the
last two *output* samples (pcport: port/src/mixer.c:190 aADPCMdecImpl, scalar path
:337-351, which is the reference this ports):

    ins[j] = (s16)(sext4(nibble_j) << shift)
    acc    = tbl[0][j]*prev2 + tbl[1][j]*prev1 + (ins[j] << 11)
             + sum_{k<j} tbl[1][j-k-1]*ins[k]
    out[j] = clamp16(acc >> 11)

Decoding starts from a zeroed state (A_INIT on the first chunk, n_load.c /
mixer.c:211). A loop's stored `state` is just the decoder history at loop.start,
so a straight linear decode reproduces the loop body; loop points are in samples.
Every bank wavetable is ADPCM order 2 (1535 x 4 predictors, 10 x 1); RAW16 is
supported anyway (big-endian s16).

# Playback rate (pitch)

The N64 synth resamples each voice by a *ratio* relative to the audio output
rate. For SFX that ratio is `state->pitch * state->basepitch`
(n_sndplayer.c:261) where `state->pitch` = the caller's pitch (1.0 by default,
lib/snd.c:2102, n_sndplayer.c:755) and

    basepitch = alCents2Ratio(keyBase*100 + detune - 6000)        n_sndplayer.c:633
    (or keyBase*100 - 6000 if KEYMAP_FLAGS has SNDSTATEFLAG_HAS_DETUNE_PITCH 0x20,
     n_sndplayer.c:631; no NTSC-final sound sets it)

`alCents2Ratio` is 2^(cents/1200) by f32 square-and-multiply
(lib/ultra/audio/cents2ratio.c:12). The resampler clamps the ratio to MAX_RATIO
1.99996 (include/PR/abi.h:261, n_resample.c:25) and quantizes it to 1/32768
(n_resample.c:32). The output rate is `osAiSetFrequency(22020)`
(lib/audiomgr.c:64), which on NTSC hardware returns
`VI_NTSC_CLOCK / round(VI_NTSC_CLOCK/22020)` = 48681812 / 2211 = **22018 Hz**
(lib/ultra/io/aisetfreq.c:9-25, include/PR/rcp.h:601). The PC port asks SDL for
22020 and its osAiSetFrequency returns the request unchanged (pcport:
port/src/audio.c:25, port/src/libultra.c:157) and runs the identical basepitch
code (pcport: src/lib/naudio/n_sndplayer.c:603-605). So:

    natural rate (Hz) = 22018 * quantize(min(basepitch, 1.99996))

keyBase 60 plays at 22018 Hz, 54 at ~15569 Hz, 48 at ~11009 Hz. The bank header's
`sampleRate` (44100) is never read by the SFX player. The bank also bakes in no
further per-sound transposition, so `pitch` in the manifest is only the
AUDIOCONFIG pitch (see below), usually 1.0.

# Volume

The voice volume is `volTable[volindex] * envvol * vol * sampleVolume / 0x3f01 /
AL_VOL_FULL` (n_sndplayer.c:253), 0x3f01 = 127*127. With the (uniform) volume
table and caller volume at full, a sound's intrinsic level is
`sampleVolume/127 * attackVolume/127`; that is the manifest `volume`. The
envelope then holds the level for `decayTime` (it is a gate: 1540/1545 sounds
have decayVolume == attackVolume == 127) and fades out over `releaseTime`; the
times are divided by the pitch ratio (n_sndplayer.c:217-222, :271, :334).
decayTime == -1 means "no decay, play until stopped" (n_sndplayer.c:620). The
manifest gives `gate_s`/`release_s` at pitch 1.0; `export --envelope` bakes them.

# Sound ids: SFXNUM vs SFXMAP (include/sfx.h:1-22, types.h:3429 union soundnumhack)

A 16-bit sound ref. Bit 15 clear: `0uummsss ssssssss` - `mm` (bits 11-12,
`mp3priority`) nonzero means an MP3 file number (snd_is_mp3, lib/snd.c:1562),
otherwise the low 11 bits are the bank sound number (`id`). `enum sfxnum`
(include/sfx.h:27) names 0x000..0x609 in order.

Bit 15 set (`enum sfxmap`, include/sfx.h:1580, SFXMAP_8000 = 0x8000): the low 15
bits index `g_AudioRussMappings[]` (lib/snd.c:171; struct audiorussmapping
types.h:3445 = { s16 soundnum; u16 audioconfig_index }). `snd_start` replaces the
ref with the entry's `soundnum` (lib/snd.c:2111) and then uses its low 11 bits
(the entry's own 0x8000 is ignored by the `id` bitfield) or plays it as an MP3.
`audioconfig_index` selects `g_AudioConfigs[]` (lib/snd.c:650; struct audioconfig
types.h:3450 = dist1..3, pitch, volpercentage, pan, volchangespeed, flags). The
config's volpercentage/pan are applied only on the `snd_start_extra` /
`snd_adjust` paths (lib/snd.c:2030, :1935) - which gunscript sounds use
(game/bondgun.c:674-690) - and volpercentage + pitch (if > 0) + distances on the
prop-sound path (`ps_create`, game/propsnd.c:772-790). A bare `snd_start` (e.g.
the player's own gunshot, game/bondgun.c:1821; shell casings,
game/casingtick.c:48) applies none of it. `snd_start` silently refuses
SFXNUM_0037 and SFXNUM_0009 (lib/snd.c:2113) - flagged `suppressed`.

All tables are read from the decomp C sources with `#if VERSION ...` resolved as
NTSC-final (VERSION_NTSC_FINAL = 2, src/include/versions.h:6).

Usage:
    python pd_sfx.py list
    python pd_sfx.py resolve <id>...
    python pd_sfx.py export <outdir> [ids...] [--weapon-set] [--envelope] [--no-chain]
    python pd_sfx.py weapon-set            # print the derived weapon sound set

ids: decimal, 0x-hex (>= 0x8000 means an SFXMAP ref), or SFXNUM_*/SFXMAP_* names.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import struct
import sys
import wave

HERE = os.path.dirname(os.path.abspath(__file__))
DECOMP = os.path.normpath(os.path.join(HERE, "..", "..", "reference", "pd-decomp"))
ASSETS = os.path.join(DECOMP, "src", "assets", "ntsc-final")
SFX_CTL = os.path.join(ASSETS, "sfx.ctl")
SFX_TBL = os.path.join(ASSETS, "sfx.tbl")

AL_ADPCM_WAVE = 0
AL_RAW16_WAVE = 1
ADPCMFBYTES = 9  # n_load.c:5
ADPCMFSIZE = 16  # abi.h:244
MAX_RATIO = 1.99996  # abi.h:261
UNITY_PITCH = 0x8000  # abi.h:260
SNDSTATEFLAG_HAS_DETUNE_PITCH = 0x20  # constants.h:3836

VI_NTSC_CLOCK = 48681812  # rcp.h:601
REQUESTED_AI_FREQ = 22020  # audiomgr.c:64


def n64_output_rate(clock: int = VI_NTSC_CLOCK, freq: int = REQUESTED_AI_FREQ) -> int:
    """osAiSetFrequency (lib/ultra/io/aisetfreq.c:4): the rate the DAC really runs at."""
    dacrate = int(f32(clock / f32(float(freq)) + 0.5))
    return clock // dacrate


def f32(x: float) -> float:
    return struct.unpack(">f", struct.pack(">f", x))[0]


def cents2ratio(cents: int) -> float:
    """alCents2Ratio (lib/ultra/audio/cents2ratio.c:12), f32 square-and-multiply."""
    ratio = 1.0
    if cents >= 0:
        x = f32(1.00057779)
    else:
        x = f32(0.9994225441)
        cents = -cents
    while cents:
        if cents & 1:
            ratio = f32(ratio * x)
        x = f32(x * x)
        cents >>= 1
    return ratio


def effective_ratio(ratio: float) -> float:
    """n_resample.c:25-33: clamp to MAX_RATIO then quantize to 1/UNITY_PITCH."""
    if ratio > MAX_RATIO:
        ratio = MAX_RATIO
    return int(ratio * UNITY_PITCH) / UNITY_PITCH


# ----------------------------------------------------------------------------
# Bank parsing
# ----------------------------------------------------------------------------


class Sound:
    __slots__ = (
        "num", "ctl_off", "pan", "volume", "sflags", "env", "keymap", "wtype",
        "base", "length", "book_order", "book_npred", "book", "loop", "basepitch",
        "ratio", "detune_flag",
    )

    @property
    def nsamples(self) -> int:
        if self.wtype == AL_ADPCM_WAVE:
            return (self.length // ADPCMFBYTES) * ADPCMFSIZE
        return self.length // 2

    @property
    def chain_next(self) -> int:
        vmin, _vmax, kmin = self.keymap[0], self.keymap[1], self.keymap[2]
        return vmin + (kmin & 0xC0) * 4  # KEYMAP_SOUNDNUM, n_sndplayer.c:16

    @property
    def delay_us(self) -> int:
        return self.keymap[1] * 33333  # KEYMAP_DELAY, n_sndplayer.c:11

    @property
    def keyflags(self) -> int:
        return self.keymap[3] & 0xF0  # KEYMAP_FLAGS, n_sndplayer.c:12


class Bank:
    def __init__(self, ctl_path: str = SFX_CTL, tbl_path: str = SFX_TBL, output_rate: int | None = None):
        with open(ctl_path, "rb") as fh:
            self.ctl = fh.read()
        with open(tbl_path, "rb") as fh:
            self.tbl = fh.read()
        self.output_rate = output_rate if output_rate else n64_output_rate()
        d = self.ctl
        revision, bankcount = struct.unpack_from(">hh", d, 0)
        if revision != 0x4231:
            raise ValueError("sfx.ctl revision %#x != 0x4231 (libaudio.h:167)" % revision)
        bank_off = struct.unpack_from(">I", d, 4)[0]  # bankArray[0]
        instcount, _bflags, _pad, self.bank_rate, _perc = struct.unpack_from(">hBBiI", d, bank_off)
        inst_off = struct.unpack_from(">I", d, bank_off + 12)[0]  # instArray[0]
        self.bankcount, self.instcount = bankcount, instcount
        soundcount = struct.unpack_from(">h", d, inst_off + 14)[0]
        self.num_sounds = soundcount + 1  # g_NumSounds, snd.c:959
        offs = struct.unpack_from(">%dI" % soundcount, d, inst_off + 16)  # snd.c:968
        self.sounds: dict[int, Sound] = {}
        for i, off in enumerate(offs):
            self.sounds[i + 1] = self._parse_sound(i + 1, off)  # soundArray[N-1], snd.c:1352

    def _parse_sound(self, num: int, off: int) -> Sound:
        d = self.ctl
        s = Sound()
        s.num, s.ctl_off = num, off
        env_off, km_off, wt_off, s.pan, s.volume, s.sflags = struct.unpack_from(">IIIBBB", d, off)
        s.env = struct.unpack_from(">iiiBB", d, env_off)
        s.keymap = struct.unpack_from(">BBBBBb", d, km_off)
        base, length, wtype, _wflags, loop_off, book_off = struct.unpack_from(">IiBBxxII", d, wt_off)
        s.wtype, s.base = wtype, base
        s.loop = None
        s.book = None
        s.book_order = s.book_npred = 0
        if wtype == AL_ADPCM_WAVE:
            s.length = ADPCMFBYTES * (length // ADPCMFBYTES)  # n_load.c:203
            s.book_order, s.book_npred = struct.unpack_from(">ii", d, book_off)
            n = s.book_order * s.book_npred * 8
            s.book = struct.unpack_from(">%dh" % n, d, book_off + 8)
            if loop_off:  # snd.c:1158 (offset 0 = no loop)
                start, end, count = struct.unpack_from(">IIi", d, loop_off)
                s.loop = (start, end, count)
        elif wtype == AL_RAW16_WAVE:
            s.length = length
            if loop_off:
                start, end, count = struct.unpack_from(">IIi", d, loop_off)
                s.loop = (start, end, count)
        else:
            raise ValueError("sound %#x: unknown wavetable type %d" % (num, wtype))
        keybase, detune = s.keymap[4], s.keymap[5]
        s.detune_flag = bool((s.keymap[3] & 0xF0) & SNDSTATEFLAG_HAS_DETUNE_PITCH)
        cents = keybase * 100 - 6000 if s.detune_flag else keybase * 100 + detune - 6000
        s.basepitch = cents2ratio(cents)  # n_sndplayer.c:631-633
        s.ratio = effective_ratio(s.basepitch)
        return s

    # -- rates --------------------------------------------------------------
    def rate_exact(self, s: Sound) -> float:
        return self.output_rate * s.ratio

    def rate(self, s: Sound) -> int:
        return int(round(self.rate_exact(s)))

    # -- decode ------------------------------------------------------------
    def decode(self, s: Sound) -> list[int]:
        data = self.tbl[s.base:s.base + s.length]
        if len(data) != s.length:
            raise ValueError("sound %#x: wave data runs past sfx.tbl" % s.num)
        if s.wtype == AL_RAW16_WAVE:
            return list(struct.unpack(">%dh" % (len(data) // 2), data[: len(data) // 2 * 2]))
        return decode_vadpcm(data, s.book, s.book_order, s.book_npred)


def decode_vadpcm(data: bytes, book, order: int, npred: int) -> list[int]:
    """Port of aADPCMdecImpl's scalar path (pcport: port/src/mixer.c:190, :337-351)."""
    if order != 2:
        raise ValueError("VADPCM order %d unsupported (only order 2 exists in PD's bank)" % order)
    # Precompute per-predictor rows: out[j] = c2[j]*prev2 + c1[j]*prev1 + sum_k m[j][k]*ins[k]
    tables = []
    for p in range(npred):
        t0 = book[p * 16: p * 16 + 8]
        t1 = book[p * 16 + 8: p * 16 + 16]
        rows = []
        for j in range(8):
            m = [t1[j - k - 1] for k in range(j)] + [2048]
            rows.append((t0[j], t1[j], m))
        tables.append(rows)
    out: list[int] = []
    prev1 = prev2 = 0
    nframes = len(data) // ADPCMFBYTES
    for f in range(nframes):
        o = f * ADPCMFBYTES
        hdr = data[o]
        shift = hdr >> 4
        idx = hdr & 0xF
        if idx >= npred:
            # Out-of-range predictor would read past the loaded book on the RSP; the
            # bank never does this (checked by `list --check`), so treat as an error.
            raise ValueError("predictor index %d >= npredictors %d" % (idx, npred))
        rows = tables[idx]
        for half in range(2):
            ins = []
            for b in data[o + 1 + half * 4: o + 5 + half * 4]:
                for nib in (b >> 4, b & 0xF):
                    v = (nib - 16 if nib >= 8 else nib) << shift
                    v &= 0xFFFF  # stored into int16_t (mixer.c:340)
                    ins.append(v - 0x10000 if v >= 0x8000 else v)
            for j in range(8):
                c0, c1, m = rows[j]
                acc = c0 * prev2 + c1 * prev1
                for k in range(j + 1):
                    acc += m[k] * ins[k]
                acc >>= 11
                if acc > 32767:
                    acc = 32767
                elif acc < -32768:
                    acc = -32768
                out.append(acc)
            prev2, prev1 = out[-2], out[-1]
    return out


# ----------------------------------------------------------------------------
# Minimal C preprocessor + table parsing for the decomp sources
# ----------------------------------------------------------------------------

CPP_DEFINES = {
    "VERSION_NTSC_BETA": 0, "VERSION_NTSC_1_0": 1, "VERSION_NTSC_FINAL": 2,  # versions.h:4-9
    "VERSION_PAL_BETA": 3, "VERSION_PAL_FINAL": 4, "VERSION_JPN_FINAL": 5,
    "VERSION": 2, "PAL": 0, "AVOID_UB": 0, "PIRACYCHECKS": 1,
}


def _cpp_eval(expr: str) -> bool:
    expr = expr.split("//")[0]
    expr = re.sub(r"/\*.*?\*/", "", expr)
    expr = re.sub(r"defined\s*\(\s*(\w+)\s*\)|defined\s+(\w+)",
                  lambda m: "1" if (m.group(1) or m.group(2)) in CPP_DEFINES else "0", expr)
    expr = re.sub(r"[A-Za-z_]\w*", lambda m: str(CPP_DEFINES.get(m.group(0), 0)), expr)
    expr = expr.replace("&&", " and ").replace("||", " or ")
    expr = re.sub(r"!(?!=)", " not ", expr)
    return bool(eval(expr, {"__builtins__": {}}, {}))  # noqa: S307 - digits/operators only


def preprocess(path: str) -> list[tuple[int, str]]:
    """Return (1-based line number, text) for lines live in the NTSC-final build."""
    with open(path, encoding="utf-8", errors="replace") as fh:
        lines = fh.read().split("\n")
    out = []
    stack: list[list[bool]] = []  # [active_now, any_branch_taken, parent_active]
    active = True
    for no, line in enumerate(lines, 1):
        st = line.strip()
        m = re.match(r"#\s*(if|ifdef|ifndef|elif|else|endif)\b(.*)", st)
        if m:
            kw, rest = m.group(1), m.group(2).strip()
            if kw in ("if", "ifdef", "ifndef"):
                if kw == "if":
                    cond = _cpp_eval(rest)
                elif kw == "ifdef":
                    cond = rest.split()[0] in CPP_DEFINES
                else:
                    cond = rest.split()[0] not in CPP_DEFINES
                stack.append([active and cond, cond, active])
            elif kw == "elif":
                top = stack[-1]
                cond = (not top[1]) and _cpp_eval(rest)
                top[0] = top[2] and cond
                top[1] = top[1] or cond
            elif kw == "else":
                top = stack[-1]
                top[0] = top[2] and not top[1]
                top[1] = True
            else:
                stack.pop()
            active = stack[-1][0] if stack else True
            continue
        if active:
            out.append((no, line))
    return out


def _strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", lambda m: re.sub(r"[^\n]", " ", m.group(0)), text, flags=re.S)
    return re.sub(r"//[^\n]*", lambda m: " " * len(m.group(0)), text)  # keep offsets


def _split_top(body: str) -> list[str]:
    parts, depth, cur = [], 0, []
    for ch in body:
        if ch in "({[":
            depth += 1
        elif ch in ")}]":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    tail = "".join(cur).strip()
    if tail:
        parts.append(tail)
    return parts


def _parse_enum(pre: list[tuple[int, str]], enum_name: str, rel: str) -> dict[str, tuple[int, str]]:
    """name -> (value, 'file:line') for a C enum (sequential values, explicit '= n' honoured)."""
    res: dict[str, tuple[int, str]] = {}
    inside, val = False, -1
    for no, line in pre:
        if not inside:
            if re.match(r"\s*enum\s+%s\s*\{" % re.escape(enum_name), line):
                inside = True
            continue
        code = _strip_comments(line).strip()
        if code.startswith("}"):
            break
        for item in filter(None, (x.strip() for x in code.split(","))):
            m = re.match(r"(\w+)\s*(?:=\s*(.+))?$", item)
            if not m:
                continue
            val = int(m.group(2), 0) if m.group(2) else val + 1
            res[m.group(1)] = (val, "%s:%d" % (rel, no))
    return res


class Symbols:
    """sfxnum/sfxmap enums + g_AudioRussMappings + g_AudioConfigs, NTSC-final."""

    def __init__(self, decomp: str = DECOMP):
        sfx_h = os.path.join(decomp, "src", "include", "sfx.h")
        snd_c = os.path.join(decomp, "src", "lib", "snd.c")
        pre_h = preprocess(sfx_h)
        self.sfxnum = _parse_enum(pre_h, "sfxnum", "src/include/sfx.h")
        self.sfxmap = _parse_enum(pre_h, "sfxmap", "src/include/sfx.h")
        self.byname = {k: v[0] for k, v in self.sfxnum.items()}
        self.byname.update({k: v[0] for k, v in self.sfxmap.items()})
        self.num_names: dict[int, list[str]] = {}
        for k, (v, _) in self.sfxnum.items():
            self.num_names.setdefault(v, []).append(k)
        self.map_names: dict[int, list[str]] = {}
        for k, (v, _) in self.sfxmap.items():
            self.map_names.setdefault(v, []).append(k)
        pre_c = preprocess(snd_c)
        self.audioconfig_idx = {k: v[0] for k, v in _parse_enum(pre_c, "audioconfig_e", "src/lib/snd.c").items()}
        self.mappings: list[dict] = []  # index -> {soundnum(packed), config, line, mp3}
        self.configs: list[dict] = []
        self._parse_tables(pre_c)
        self.warnings: list[str] = []
        self._self_check()

    def _eval_ref(self, expr: str) -> int | None:
        expr = expr.strip()
        if re.search(r"\bMP3\w*\(", expr):
            return None
        expr = re.sub(r"\b(SFX(?:NUM|MAP)_\w+)\b", lambda m: str(self.byname[m.group(1)]), expr)
        if not re.fullmatch(r"[0-9xXa-fA-F|\s()+]+", expr):
            raise ValueError("cannot evaluate sound ref %r" % expr)
        return eval(expr, {"__builtins__": {}}, {})  # noqa: S307 - digits/operators only

    def _parse_tables(self, pre: list[tuple[int, str]]) -> None:
        mode = None
        for no, line in pre:
            if "struct audiorussmapping g_AudioRussMappings[]" in line:
                mode = "map"
                continue
            if "struct audioconfig g_AudioConfigs[]" in line:
                mode = "cfg"
                continue
            if mode is None:
                continue
            if line.startswith("};"):
                mode = None
                continue
            m = re.search(r"\{(.*)\}", _strip_comments(line))
            if not m:
                continue
            fields = _split_top(m.group(1))
            if fields == ["0"]:
                if mode == "map":
                    mode = None  # { 0 } terminator
                continue
            if mode == "map":
                c = re.search(r"/\*0x([0-9a-fA-F]+)\*/", line)
                ref = self._eval_ref(fields[0])
                self.mappings.append({
                    "packed": ref, "config": self.audioconfig_idx[fields[1]], "config_name": fields[1],
                    "line": no, "comment_index": int(c.group(1), 16) if c else None,
                    "expr": fields[0], "mp3": ref is None or bool((ref >> 11) & 3),
                })
            else:
                d1, d2, d3, pitch, volp, pan, vcs = (float(x) for x in fields[:7])
                self.configs.append({
                    "dist": [d1, d2, d3], "pitch": pitch, "volpercentage": int(volp),
                    "pan": int(pan), "volchangespeed": int(vcs), "flags": fields[7], "line": no,
                })

    def _self_check(self) -> None:
        # Enum names carry their NTSC-final value in hex; the resolved enum must agree.
        for table in (self.sfxnum, self.sfxmap):
            for name, (val, where) in table.items():
                m = re.match(r"SFX(?:NUM|MAP)_([0-9A-F]{4})(?:_|$)", name)
                if m and int(m.group(1), 16) != val:
                    self.warnings.append("%s = %#x at %s (name says %s)" % (name, val, where, m.group(1)))
        for i, mp in enumerate(self.mappings):
            if mp["comment_index"] is not None and mp["comment_index"] != i:
                self.warnings.append("g_AudioRussMappings line %d is index %#x, comment says %#x"
                                     % (mp["line"], i, mp["comment_index"]))

    def parse_id(self, text: str) -> int:
        t = text.strip()
        if t in self.byname:
            return self.byname[t]
        if re.fullmatch(r"0[xX][0-9a-fA-F]+", t):
            return int(t, 16)
        if re.fullmatch(r"\d+", t):
            return int(t)
        raise ValueError("unknown sound id %r" % text)

    def resolve(self, ref: int) -> dict:
        """Follow snd_start (lib/snd.c:2089): returns {num|mp3, config, chain of provenance}."""
        info: dict = {"ref": ref, "num": None, "mp3": False, "config": None, "prov": []}
        packed = ref & 0xFFFF
        if packed & 0x8000:
            idx = packed & 0x7FFF
            if idx >= len(self.mappings):
                raise ValueError("SFXMAP index %#x beyond g_AudioRussMappings" % idx)
            mp = self.mappings[idx]
            info["config"] = mp["config"]
            info["prov"].append("g_AudioRussMappings[%#06x] src/lib/snd.c:%d = {%s, %s} (remap: snd.c:2111)"
                                % (idx, mp["line"], mp["expr"], mp["config_name"]))
            if mp["packed"] is None:
                info["mp3"] = True
                return info
            packed = mp["packed"] & 0xFFFF
        if (packed >> 11) & 3:  # mp3priority, snd_is_mp3 snd.c:1562
            info["mp3"] = True
            return info
        info["num"] = packed & 0x7FF  # soundnumhack.id, types.h:3441
        return info


# ----------------------------------------------------------------------------
# Weapon sound set (derived from the decomp)
# ----------------------------------------------------------------------------

SOUND_TOKEN = re.compile(r"\b(SFX(?:NUM|MAP)_\w+)\b")


def _function_body(pre: list[tuple[int, str]], signature: str) -> list[tuple[int, str]]:
    start = None
    for i, (_no, line) in enumerate(pre):
        if line.startswith(signature):
            start = i
            break
    if start is None:
        raise ValueError("function %r not found" % signature)
    depth, seen, body = 0, False, []
    for no, line in pre[start:]:
        code = _strip_comments(line)
        body.append((no, line))
        depth += code.count("{") - code.count("}")
        if "{" in code:
            seen = True
        if seen and depth == 0:
            break
    return body


def weapon_set(sym: Symbols, decomp: str = DECOMP) -> list[tuple[str, str]]:
    """[(sound symbol, usage string)] from invitems.c, bondgun.c, tex.c, casingtick.c."""
    uses: list[tuple[str, str]] = []
    src = os.path.join(decomp, "src", "game")
    # invitems.c: funcdef shootsound (element 20 of struct funcdef_shoot, types.h:2941)
    # and shootprojectile soundnum (element 30, types.h:2963), gunscript_playsound.
    pre = preprocess(os.path.join(src, "invitems.c"))
    text = "\n".join(line for _no, line in pre)
    line_of = []
    for no, line in pre:
        line_of.extend([no] * (len(line) + 1))
    clean = _strip_comments(text)
    for m in re.finditer(r"struct\s+(funcdef_shoot\w*)\s+(\w+)\s*=\s*\{(.*?)\n\};", clean, re.S):
        kind, name, body = m.group(1), m.group(2), m.group(3)
        fields = _split_top(body)
        want = [(20, "shootsound")]
        if kind == "funcdef_shootprojectile":
            want.append((30, "projectile soundnum"))
        for idx, label in want:
            val = fields[idx].strip() if idx < len(fields) else ""
            if SOUND_TOKEN.fullmatch(val):
                pos = m.start(3) + body.find(val)
                uses.append((val, "invitems.c:%d %s %s" % (line_of[pos], name, label)))
    script = None
    speed = None
    for no, line in pre:
        s = re.match(r"struct\s+guncmd\s+(\w+)\[\]", line)
        if s:
            script, speed = s.group(1), None
        sp = re.search(r"gunscript_setsoundspeed\(\s*(\d+)\s*,\s*(\d+)\s*\)", line)
        if sp:
            speed = int(sp.group(2)) / 1000.0
        g = re.search(r"gunscript_playsound\(\s*(\d+)\s*,\s*(\w+)\s*\)", line)
        if g:
            extra = " speed=%.3g" % speed if speed else ""
            speed = None  # consumed by the next PLAYSOUND (bondgun.c:676-679)
            uses.append((g.group(2), "invitems.c:%d %s gunscript_playsound kf%s%s"
                         % (no, script, g.group(1), extra)))
    # bondgun.c hit sounds (NTSC-final branch only, via preprocess)
    pre = preprocess(os.path.join(src, "bondgun.c"))
    for sig in ("void bgun_play_prop_hit_sound(", "void bgun_play_bg_hit_sound(",
                "void bgun_play_glass_hit_sound("):
        for no, line in _function_body(pre, sig):
            for tok in SOUND_TOKEN.findall(_strip_comments(line)):
                uses.append((tok, "bondgun.c:%d %s" % (no, sig[5:-1])))
    # tex.c g_SurfaceTypes sound lists (bondgun hit functions play type->sounds[])
    for no, line in preprocess(os.path.join(src, "tex.c")):
        m = re.match(r"u16\s+(g_SurfaceType\w+Sounds)\[\]", line)
        if m:
            for tok in SOUND_TOKEN.findall(line):
                uses.append((tok, "tex.c:%d %s (bullet impact surface)" % (no, m.group(1))))
    # casingtick.c: shell casing landing (random pitch 0.98..1.23, casingtick.c:45)
    for no, line in preprocess(os.path.join(src, "casingtick.c")):
        for tok in SOUND_TOKEN.findall(_strip_comments(line)):
            uses.append((tok, "casingtick.c:%d casing drop, pitch 0.98-1.23 random" % no))
    return uses


# ----------------------------------------------------------------------------
# Output
# ----------------------------------------------------------------------------


def write_wav(path: str, samples: list[int], rate: int) -> None:
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(struct.pack("<%dh" % len(samples), *samples))


def envelope_times(s: Sound) -> tuple[float | None, float]:
    """(gate seconds or None for no-decay, release seconds) at pitch 1.0 (n_sndplayer.c:217-334)."""
    attack, decay, release = s.env[0], s.env[1], s.env[2]
    rel = release / 1e6 / s.basepitch
    if decay == -1:
        return None, rel
    return (attack + decay) / 1e6 / s.basepitch, rel


def apply_envelope(s: Sound, samples: list[int], rate: float) -> list[int]:
    """Bake the sndplayer gate: level attackVolume..decayVolume, then linear release."""
    gate, rel = envelope_times(s)
    if gate is None:
        return samples
    attack_us, decay_us = s.env[0], s.env[1]
    av, dv = s.env[3], s.env[4]
    n_att = int(attack_us / 1e6 / s.basepitch * rate)
    n_gate = int(gate * rate)
    n_rel = int(rel * rate)
    out = []
    for i, v in enumerate(samples):
        if i < n_att:
            g = av / 127.0 * (i / max(n_att, 1))
        elif i < n_gate:
            t = (i - n_att) / max(n_gate - n_att, 1)
            g = (av + (dv - av) * t) / 127.0
        elif i < n_gate + n_rel:
            g = dv / 127.0 * (1.0 - (i - n_gate) / max(n_rel, 1))
        else:
            break
        out.append(int(round(v * g)))
    return out


def sound_entry(bank: Bank, sym: Symbols, s: Sound, envelope_baked: bool) -> dict:
    gate, rel = envelope_times(s)
    loop = None
    if s.loop:
        loop = {"start": s.loop[0], "end": s.loop[1], "count": s.loop[2]}
    e = {
        "file": "%04x.wav" % s.num,
        "sound": "%04x" % s.num,
        "rate": bank.rate(s),
        "rate_exact": round(bank.rate_exact(s), 3),
        "samples": s.nsamples,
        "duration_s": round(s.nsamples / bank.rate_exact(s), 4),
        "loop": loop,
        "pitch": 1.0,
        "volume": round(s.volume / 127.0 * s.env[3] / 127.0, 4),
        "gate_s": None if gate is None else round(gate, 4),
        "release_s": round(rel, 4),
        "envelope_baked": envelope_baked,
        "source": ("sfx.ctl soundArray[%#x] @ctl+%#x (snd.c:1352); tbl+%#x len %d; "
                   "rate = %d * alCents2Ratio(keyBase %d*100 %+d - 6000) (n_sndplayer.c:633, audiomgr.c:64); "
                   "volume = sampleVolume %d/127 * attackVolume %d/127 (n_sndplayer.c:253)"
                   % (s.num - 1, s.ctl_off, s.base, s.length, bank.output_rate, s.keymap[4],
                      s.keymap[5], s.volume, s.env[3])),
    }
    if s.num in (0x37, 0x09):
        e["suppressed"] = "snd_start refuses this id (lib/snd.c:2113); the game never plays it"
    if s.delay_us:
        # KEYMAP_DELAY: the game starts this sound delay_s after snd_start (n_sndplayer.c:763,771)
        e["delay_s"] = round(s.delay_us / 1e6, 5)
    if s.chain_next:
        # KEYMAP_SOUNDNUM: playing this also plays these, each delay_s after the trigger
        # (n_sndplayer.c:763-779). Not mixed into the WAV.
        chain = []
        cur, seen = bank.sounds.get(s.chain_next), {s.num}
        while cur and cur.num not in seen:
            seen.add(cur.num)
            chain.append({"sound": "%04x" % cur.num, "file": "%04x.wav" % cur.num,
                          "delay_s": round(cur.delay_us / 1e6, 5)})
            cur = bank.sounds.get(cur.chain_next) if cur.chain_next else None
        e["chain"] = chain
    names = sym.num_names.get(s.num)
    if names:
        e["names"] = names
    return e


def resolve_request(sym: Symbols, text: str) -> tuple[int, dict]:
    ref = sym.parse_id(text)
    return ref, sym.resolve(ref)


def cmd_list(args) -> int:
    bank = Bank(output_rate=args.output_rate)
    sym = Symbols()
    print("# %d sounds, output rate %d Hz (bank header sampleRate %d is unused)"
          % (len(bank.sounds), bank.output_rate, bank.bank_rate))
    print("num   samples   rate  loop  name")
    for n in sorted(bank.sounds):
        s = bank.sounds[n]
        name = (sym.num_names.get(n) or [""])[0]
        print("%04x %8d %6d  %-4s  %s" % (n, s.nsamples, bank.rate(s), "yes" if s.loop else "no", name))
    if args.check:
        bad = 0
        for n in sorted(bank.sounds):
            s = bank.sounds[n]
            if s.wtype == AL_ADPCM_WAVE:
                data = bank.tbl[s.base:s.base + s.length]
                for f in range(0, len(data), ADPCMFBYTES):
                    if (data[f] & 0xF) >= s.book_npred:
                        bad += 1
                        break
        print("# sounds with out-of-range predictor index: %d" % bad)
        for w in sym.warnings:
            print("# WARN", w)
    return 0


def cmd_resolve(args) -> int:
    bank = Bank(output_rate=args.output_rate)
    sym = Symbols()
    for t in args.ids:
        ref, info = resolve_request(sym, t)
        if info["mp3"]:
            print("%s (%#06x) -> MP3 (not in sfx.ctl)" % (t, ref))
            continue
        s = bank.sounds.get(info["num"])
        cfg = sym.configs[info["config"]] if info["config"] is not None else None
        print("%s (%#06x) -> sound %04x %s rate %d samples %d%s%s" % (
            t, ref, info["num"], (sym.num_names.get(info["num"]) or [""])[0],
            bank.rate(s) if s else 0, s.nsamples if s else 0,
            " config %d vol%% %d pitch %g" % (info["config"], cfg["volpercentage"], cfg["pitch"]) if cfg else "",
            "  [" + "; ".join(info["prov"]) + "]" if info["prov"] else ""))
    return 0


def cmd_weapon_set(args) -> int:
    sym = Symbols()
    for tok, use in weapon_set(sym):
        info = sym.resolve(sym.byname[tok])
        print("%-34s -> %s   %s" % (tok, "MP3" if info["mp3"] else "%04x" % info["num"], use))
    return 0


def cmd_export(args) -> int:
    bank = Bank(output_rate=args.output_rate)
    sym = Symbols()
    os.makedirs(args.outdir, exist_ok=True)
    requests: list[tuple[str, str | None]] = [(t, None) for t in args.ids]
    if args.weapon_set:
        requests += weapon_set(sym)
    if not requests:
        requests = [("0x%x" % n, None) for n in sorted(bank.sounds)]
        requests += [(name, None) for name in sym.sfxnum if 0 < sym.sfxnum[name][0] < bank.num_sounds]
        requests += [(name, None) for name in sym.sfxmap if not name.endswith("_END")]
    manifest: dict[str, dict] = {}
    targets: dict[int, dict] = {}
    unresolved: list[str] = []
    mp3s: list[str] = []  # SFXMAP entries / refs that are MP3 speech files, not bank sounds
    aliases: dict[str, tuple[int, dict, list[str]]] = {}
    for text, use in requests:
        try:
            ref, info = resolve_request(sym, text)
        except ValueError as exc:
            unresolved.append("%s: %s" % (text, exc))
            continue
        if info["mp3"] or info["num"] is None:
            mp3s.append(text)
            continue
        num = info["num"]
        if num not in bank.sounds:
            unresolved.append("%s: sound %#x out of range (1..%#x)" % (text, num, bank.num_sounds - 1))
            continue
        targets[num] = info
        keys = []
        if not re.fullmatch(r"0[xX][0-9a-fA-F]+|\d+", text):
            keys.append(text)
        elif ref & 0x8000:
            keys.extend(sym.map_names.get(ref, []))
        keys.extend(sym.num_names.get(num, []))
        for k in keys:
            a = aliases.setdefault(k, (ref if k.startswith("SFXMAP_") else num, info, []))
            if use:
                a[2].append(use)
    if not args.no_chain:
        for num in list(targets):
            s = bank.sounds[num]
            seen = {num}
            while s.chain_next and s.chain_next not in seen and s.chain_next in bank.sounds:
                seen.add(s.chain_next)
                targets.setdefault(s.chain_next, {"num": s.chain_next, "config": None, "prov": []})
                s = bank.sounds[s.chain_next]
    total = 0
    for num in sorted(targets):
        s = bank.sounds[num]
        samples = bank.decode(s)
        rate = bank.rate(s)
        if args.envelope and not s.loop:
            samples = apply_envelope(s, samples, bank.rate_exact(s))
        path = os.path.join(args.outdir, "%04x.wav" % num)
        write_wav(path, samples, rate)
        total += os.path.getsize(path)
        manifest["%04x" % num] = sound_entry(bank, sym, s, bool(args.envelope and not s.loop))
    for key, (ref, info, uses) in sorted(aliases.items()):
        e = dict(manifest["%04x" % info["num"]])
        prov = list(info["prov"])
        if key.startswith("SFXMAP_") and info["config"] is not None:
            cfg = sym.configs[info["config"]]
            e["pitch"] = cfg["pitch"] if cfg["pitch"] > 0 else 1.0
            e["volume"] = round(e["volume"] * cfg["volpercentage"] / 100.0, 4)
            e["config"] = {
                "index": info["config"], "line": "src/lib/snd.c:%d" % cfg["line"],
                "volpercentage": cfg["volpercentage"], "pitch": cfg["pitch"], "pan": cfg["pan"],
                "dist": cfg["dist"], "flags": cfg["flags"],
                "applies_via": "ps_create (vol%, pitch, dist: propsnd.c:772-790); snd_start_extra/snd_adjust "
                               "(vol%, pan: snd.c:2030/:1935); NOT bare snd_start",
            }
        where = sym.sfxnum.get(key) or sym.sfxmap.get(key)
        e["source"] = "%s = %#06x (%s); %s%s" % (
            key, ref, where[1] if where else "?", "; ".join(prov) + "; " if prov else "", e["source"])
        if uses:
            e["used_by"] = sorted(set(uses))
        manifest[key] = e
    mpath = os.path.join(args.outdir, "sfx_manifest.json")
    with open(mpath, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(manifest, fh, indent=1, sort_keys=True)
        fh.write("\n")
    print("wrote %d WAVs (%.2f MB) + %s (%d keys)" % (len(targets), total / 1e6, mpath, len(manifest)))
    for u in unresolved:
        print("UNRESOLVED", u)
    if mp3s:
        explicit = [t for t in mp3s if t in args.ids]
        print("skipped %d refs that are MP3 speech files, not sfx.ctl sounds%s"
              % (len(mp3s), (": " + " ".join(explicit)) if explicit else ""))
    for w in sym.warnings:
        print("WARN", w)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output-rate", type=int, default=None,
                    help="audio output rate the pitch ratio is relative to (default: N64 NTSC 22018)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("list", help="one line per sound number")
    p.add_argument("--check", action="store_true", help="also validate predictors + symbol tables")
    p.set_defaults(fn=cmd_list)
    p = sub.add_parser("resolve", help="show how ids resolve to bank sounds")
    p.add_argument("ids", nargs="+")
    p.set_defaults(fn=cmd_resolve)
    p = sub.add_parser("weapon-set", help="print the weapon sound set derived from the decomp")
    p.set_defaults(fn=cmd_weapon_set)
    p = sub.add_parser("export", help="write <outdir>/NNNN.wav + sfx_manifest.json")
    p.add_argument("outdir")
    p.add_argument("ids", nargs="*")
    p.add_argument("--weapon-set", action="store_true", help="add the derived weapon sound set")
    p.add_argument("--envelope", action="store_true",
                   help="bake the in-game gate/release envelope into non-looping sounds")
    p.add_argument("--no-chain", action="store_true", help="do not also export chained sounds")
    p.set_defaults(fn=cmd_export)
    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
