#!/usr/bin/env python3
"""Perfect Dark's music: the instrument bank (seq.ctl + seq.tbl) and the compact
MIDI sequences, into `assets/music/`. Stdlib only.

    python tools/pd-assets/pd_music.py            # build_assets.py runs it too

Everything the game's synth needs, in PD's own structures and ids:

* `music/bank.json`: seq.ctl's one bank (`snd_init` loads `bankArray[0]`,
  lib/snd.c:1486-1492, and every seq player plays from it, `seq_init`
  lib/snd.c:1398): the instruments in `instArray` order, their sounds (envelope
  and keymap inline) and the wavetables they share (ADPCM book, loop), with each
  wavetable's `base` an offset into seq.tbl (`alBnkfNew(bankfile,
  &_seqtblSegmentRomStart)`, lib/snd.c:1487). The structs are
  include/PR/libaudio.h's (see pd_sfx.py's docstring for the layouts); a pointer
  of 0 is "none".
* `music/seq.tbl`: the sample data, byte for byte.
* `music/seq/<num>.seq`: each sequence as `seq_play` has it after `rzip_inflate`
  (lib/snd.c:1606): an `ALCMidiHdr` (16 track offsets + division) and the
  compressed MIDI tracks. `<num>` is its index in `g_SeqTable`, which is its
  `MUSIC_*` number (the build makes that enum from sequences.json, in order).
* `music/index.json`: per sequence its `MUSIC_*` name, the extract's file name,
  its `g_SeqVolumes` entry (lib/snd.c:722, NTSC final) and a cross-check the
  Rust reader must reproduce (this reader's walk: note-ons, events, ticks to
  the end with every loop taken once, `n_alCSeqNextEvent(seq, evt, 0)`); plus a
  few wavetables decoded by pd_sfx.py's VADPCM decoder (hashed) for the Rust
  decoder's test.

Nothing here is resampled or mixed: the synth (`n64::naudio`) plays the bank
as the N64 does.
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

import pd_sfx  # noqa: E402
from pd_paths import asset, decomp_rel, out, require_decomp, src  # noqa: E402

SEQ_CTL = asset("seq.ctl")
SEQ_TBL = asset("seq.tbl")
SEQUENCES_JSON = asset("sequences.json")

AL_VOL_FULL = 0x7FFF  # libaudio.h:78
MAX_SEQ_SIZE_8MB = 1024 * 18  # lib/snd.c:27

AL_MIDI_META = 0xFF
AL_MIDI_META_TEMPO = 0x51
AL_MIDI_META_EOT = 0x2F
AL_CMIDI_LOOPSTART_CODE = 0x2E
AL_CMIDI_LOOPEND_CODE = 0x2D
AL_CMIDI_BLOCK_CODE = 0xFE


# ----------------------------------------------------------------------------
# The bank
# ----------------------------------------------------------------------------


def read_bank(ctl: bytes) -> dict:
    """seq.ctl as JSON: `ALBankFile` -> bankArray[0] -> instruments, sounds, wavetables."""
    revision, bankcount = struct.unpack_from(">hh", ctl, 0)
    if revision != 0x4231:
        raise ValueError("seq.ctl revision %#x != 0x4231 (libaudio.h:167)" % revision)
    bank_off = struct.unpack_from(">I", ctl, 4)[0]
    instcount, bflags, _pad, rate, percussion = struct.unpack_from(">hBBiI", ctl, bank_off)
    inst_offs = struct.unpack_from(">%dI" % instcount, ctl, bank_off + 12)

    sounds: list[dict] = []
    sound_ix: dict[int, int] = {}
    tables: list[dict] = []
    table_ix: dict[int, int] = {}

    def wavetable(off: int) -> int:
        if off in table_ix:
            return table_ix[off]
        base, length, wtype, wflags, loop_off, book_off = struct.unpack_from(">IiBBxxII", ctl, off)
        t = {"base": base, "len": length, "type": wtype, "flags": wflags, "loop": None, "book": None}
        if wtype == pd_sfx.AL_ADPCM_WAVE:
            order, npred = struct.unpack_from(">ii", ctl, book_off)
            t["book"] = {"order": order, "npredictors": npred,
                         "book": list(struct.unpack_from(">%dh" % (order * npred * 8), ctl, book_off + 8))}
            if loop_off:
                start, end, count = struct.unpack_from(">IIi", ctl, loop_off)
                state = list(struct.unpack_from(">16h", ctl, loop_off + 12))
                t["loop"] = {"start": start, "end": end, "count": count, "state": state}
        elif wtype == pd_sfx.AL_RAW16_WAVE:
            if loop_off:
                start, end, count = struct.unpack_from(">IIi", ctl, loop_off)
                t["loop"] = {"start": start, "end": end, "count": count}
        else:
            raise ValueError("wavetable at %#x: unknown type %d" % (off, wtype))
        table_ix[off] = len(tables)
        tables.append(t)
        return table_ix[off]

    def sound(off: int) -> int:
        if off in sound_ix:
            return sound_ix[off]
        env_off, km_off, wt_off, pan, volume, sflags = struct.unpack_from(">IIIBBB", ctl, off)
        at, dt, rt, av, dv = struct.unpack_from(">iiiBB", ctl, env_off)
        vmin, vmax, kmin, kmax, kbase, detune = struct.unpack_from(">BBBBBb", ctl, km_off)
        s = {
            "envelope": {"attackTime": at, "decayTime": dt, "releaseTime": rt, "attackVolume": av, "decayVolume": dv},
            "keyMap": {"velocityMin": vmin, "velocityMax": vmax, "keyMin": kmin, "keyMax": kmax,
                       "keyBase": kbase, "detune": detune},
            "wavetable": wavetable(wt_off),
            "samplePan": pan,
            "sampleVolume": volume,
            "flags": sflags,
        }
        sound_ix[off] = len(sounds)
        sounds.append(s)
        return sound_ix[off]

    def instrument(off: int) -> dict:
        (volume, pan, priority, flags, tremtype, tremrate, tremdepth, tremdelay,
         vibtype, vibrate, vibdepth, vibdelay) = struct.unpack_from(">12B", ctl, off)
        bendrange, soundcount = struct.unpack_from(">hh", ctl, off + 12)
        return {
            "volume": volume, "pan": pan, "priority": priority, "flags": flags,
            "tremType": tremtype, "tremRate": tremrate, "tremDepth": tremdepth, "tremDelay": tremdelay,
            "vibType": vibtype, "vibRate": vibrate, "vibDepth": vibdepth, "vibDelay": vibdelay,
            "bendRange": bendrange,
            "sounds": [sound(s) for s in struct.unpack_from(">%dI" % soundcount, ctl, off + 16)],
        }

    instruments = [instrument(o) if o else None for o in inst_offs]
    return {
        "source": decomp_rel(SEQ_CTL),
        "bankCount": bankcount,
        "flags": bflags,
        "sampleRate": rate,
        "percussion": instrument(percussion) if percussion else None,
        "instruments": instruments,
        "sounds": sounds,
        "wavetables": tables,
    }


# ----------------------------------------------------------------------------
# The sequences
# ----------------------------------------------------------------------------


class CSeq:
    """A reader of PD's compact MIDI, as lib/naudio/n_csq.c walks it."""

    def __init__(self, data: bytes):
        self.d = bytearray(data)  # the loop-end code writes its count back (n_csq.c:117)
        self.offs = struct.unpack_from(">16I", data, 0)
        self.division = struct.unpack_from(">I", data, 64)[0]
        self.cur = [0] * 16
        self.bu_ptr = [0] * 16
        self.bu_len = [0] * 16
        self.last = [0] * 16
        self.delta = [0] * 16
        self.valid = 0
        for t, o in enumerate(self.offs):
            if o:
                self.valid |= 1 << t
                self.cur[t] = o
                self.delta[t] = self.varlen(t)
        self.last_delta = 0
        self.ticks = 0

    def at(self, p: int) -> int:
        """A byte of the sequence. The end of a sequence reads one delta past its
        last event (n_csq.c:66); PD reads whatever follows in its buffer, this
        reads 0 (as the Rust reader does)."""
        return self.d[p] if 0 <= p < len(self.d) else 0

    def byte(self, t: int) -> int:  # __getTrackByte, n_csq.c:278
        if self.bu_len[t]:
            b = self.at(self.bu_ptr[t])
            self.bu_ptr[t] += 1
            self.bu_len[t] -= 1
            return b
        b = self.at(self.cur[t])
        self.cur[t] += 1
        if b == AL_CMIDI_BLOCK_CODE:
            nb = self.at(self.cur[t])
            self.cur[t] += 1
            if nb != AL_CMIDI_BLOCK_CODE:
                lo = self.at(self.cur[t])
                ln = self.at(self.cur[t] + 1)
                self.cur[t] += 2
                self.bu_ptr[t] = self.cur[t] - (((nb << 8) + lo) + 4)
                self.bu_len[t] = ln
                b = self.at(self.bu_ptr[t])
                self.bu_ptr[t] += 1
                self.bu_len[t] -= 1
        return b

    def varlen(self, t: int) -> int:  # __readVarLen, n_csq.c:318
        v = self.byte(t)
        if v & 0x80:
            v &= 0x7F
            while True:
                c = self.byte(t)
                v = (v << 7) + (c & 0x7F)
                if not c & 0x80:
                    break
        return v

    def next_event(self) -> tuple[str, int]:
        """n_alCSeqNextEvent(seq, evt, 0) (n_csq.c:50): (event type, status byte)."""
        first, track = 0xFFFFFFFF, 0
        for t in range(16):
            if (self.valid >> t) & 1:
                self.delta[t] -= self.last_delta
                if self.delta[t] < first:
                    first, track = self.delta[t], t
        kind, status = self.track_event(track)
        self.ticks += first
        self.last_delta = first
        if kind != "track_end":
            self.delta[track] += self.varlen(track)
        return kind, status

    def track_event(self, t: int) -> tuple[str, int]:  # __n_alCSeqGetTrackEvent, n_csq.c:73
        status = self.byte(t)
        if status == AL_MIDI_META:
            ty = self.byte(t)
            if ty == AL_MIDI_META_TEMPO:
                for _ in range(3):
                    self.byte(t)
                self.last[t] = 0
                return "tempo", status
            if ty == AL_MIDI_META_EOT:
                self.valid ^= 1 << t
                return ("track_end" if self.valid else "seq_end"), status
            if ty == AL_CMIDI_LOOPSTART_CODE:
                self.byte(t)
                self.byte(t)
                self.last[t] = 0
                return "loop_start", status
            if ty == AL_CMIDI_LOOPEND_CODE:
                p = self.cur[t]
                # arg3 == 0: never loop back, reset the count (n_csq.c:118).
                self.d[p + 1] = self.d[p]
                self.cur[t] = p + 1 + 5
                self.last[t] = 0
                return "loop_end", status
            raise ValueError("unknown meta %#x" % ty)
        if status & 0x80:
            st = (status & 0xF0) | t
            self.byte(t)
            self.last[t] = st
        else:
            st = self.last[t]
        if (st & 0xF0) not in (0xC0, 0xD0):
            self.byte(t)
            if (st & 0xF0) == 0x90:
                self.varlen(t)
        return "midi", st


def walk(data: bytes) -> dict:
    """The cross-check: counts from start to `AL_SEQ_END_EVT`, loops not taken."""
    seq = CSeq(data)
    notes = events = 0
    while True:
        kind, st = seq.next_event()
        events += 1
        if kind == "midi" and (st & 0xF0) == 0x90:
            notes += 1
        if kind == "seq_end":
            break
    return {"events": events, "notes": notes, "ticks": seq.ticks, "division": seq.division}


def seq_volumes() -> list[int]:
    """`g_SeqVolumes[]` (lib/snd.c:722), NTSC final: `AL_VOL_FULL * x` truncated to s16."""
    text = open(src("lib", "snd.c"), encoding="utf-8").read()
    body = text[text.index("s16 g_SeqVolumes[] = {"):]
    body = body[body.index("{") + 1: body.index("};")]
    body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
    vols = []
    for item in [x.strip() for x in body.split(",") if x.strip()]:
        m = re.fullmatch(r"AL_VOL_FULL \* \(VERSION >= VERSION_NTSC_1_0 \? ([\d.]+) : [\d.]+\)", item)
        if m:
            vols.append(int(AL_VOL_FULL * float(m.group(1))))
            continue
        m = re.fullmatch(r"AL_VOL_FULL \* ([\d.]+)", item)
        if m:
            vols.append(int(AL_VOL_FULL * float(m.group(1))))
            continue
        vols.append(int(item))
    return vols


def hash16(samples: list[int]) -> int:
    """A hash both sides can compute: h = h * 31 + (s & 0xffff), mod 2^32."""
    h = 0
    for s in samples:
        h = (h * 31 + (s & 0xFFFF)) & 0xFFFFFFFF
    return h


def adpcm_checks(bank: dict, tbl: bytes) -> list[dict]:
    """A few wavetables decoded by pd_sfx.decode_vadpcm (pcport's scalar aADPCMdec)."""
    picks = []
    tables = bank["wavetables"]
    four = [i for i, t in enumerate(tables) if t["book"]["npredictors"] == 4]
    one = [i for i, t in enumerate(tables) if t["book"]["npredictors"] == 1]
    looped = [i for i, t in enumerate(tables) if t["loop"]]
    for i in sorted({four[0], four[-1], one[0], looped[0]}):
        t = tables[i]
        length = pd_sfx.ADPCMFBYTES * (t["len"] // pd_sfx.ADPCMFBYTES)  # n_load.c:203
        b = t["book"]
        pcm = pd_sfx.decode_vadpcm(tbl[t["base"]:t["base"] + length], b["book"], b["order"], b["npredictors"])
        picks.append({"wavetable": i, "samples": len(pcm), "first": pcm[:32], "hash": hash16(pcm)})
    return picks


# ----------------------------------------------------------------------------
# Export
# ----------------------------------------------------------------------------


def export_all(outdir: str) -> dict:
    """Write `assets/music/`. Returns counts for MANIFEST.json."""
    require_decomp()
    shutil.rmtree(outdir, ignore_errors=True)
    os.makedirs(os.path.join(outdir, "seq"), exist_ok=True)
    ctl = open(SEQ_CTL, "rb").read()
    tbl = open(SEQ_TBL, "rb").read()
    bank = read_bank(ctl)
    with open(os.path.join(outdir, "bank.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(bank, fh, indent=0)
        fh.write("\n")
    with open(os.path.join(outdir, "seq.tbl"), "wb") as fh:
        fh.write(tbl)

    vols = seq_volumes()
    seqs = json.load(open(SEQUENCES_JSON, encoding="utf-8"))
    index = []
    for num, e in enumerate(seqs):
        data = open(asset("sequences", e["file"]), "rb").read()
        if len(data) + 16 + 0x40 >= MAX_SEQ_SIZE_8MB:
            raise ValueError("%s: %d bytes won't fit seq_play's buffer (lib/snd.c:1627)" % (e["file"], len(data)))
        rel = "seq/%02x.seq" % num
        with open(os.path.join(outdir, rel), "wb") as fh:
            fh.write(data)
        index.append({"num": num, "id": e["id"], "file": e["file"], "seq": rel, "volume": vols[num],
                      "bytes": len(data), "check": walk(data)})
    with open(os.path.join(outdir, "index.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump({"source": decomp_rel(SEQUENCES_JSON), "volumes": decomp_rel(src("lib", "snd.c")),
                   "sequences": index, "adpcm_checks": adpcm_checks(bank, tbl)}, fh, indent=1)
        fh.write("\n")
    return {"music_sequences": len(index), "music_instruments": len(bank["instruments"]),
            "music_sounds": len(bank["sounds"]), "music_wavetables": len(bank["wavetables"])}


def main() -> int:
    counts = export_all(out("music"))
    print("pd_music: %s -> %s" % (json.dumps(counts), out("music")))
    return 0


if __name__ == "__main__":
    sys.exit(main())
