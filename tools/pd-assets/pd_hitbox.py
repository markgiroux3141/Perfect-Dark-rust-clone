#!/usr/bin/env python3
"""Export each Perfect Dark body's hit boxes beside its GLB, for shots that hit
body parts the way PD's do.

A PD chr model carries `BBOX` nodes (node type 0x0a, `struct modelrodata_bbox`,
`types.h:470`: `s32 hitpart; f32 xmin, xmax, ymin, ymax, zmin, zmax`). When the
player shoots a chr, `bg_test_hit_on_chr` tests the shot against each of them in
the space of the matrix that poses it -- the nearest `POSITION` ancestor's slot 0
(`model_find_node_mtx`) -- and the box's `hitpart` decides the damage multiplier
in `chr_damage` (`chraction.c:4722`: head x4, torso x2, gun 0, the rest x1).

`pd_gltf.py` writes a body's vertices in the same bone-local space (identity
inverse-binds, see its docstring), so a box needs only the same unit scale
(`EXPORT_SCALE`) and the rig joint its matrix slot maps to. A grafted head's
boxes ride the head joint, as its vertices do (`attach_head`).

Output, per roster character: `<outdir>/<name>.hitboxes.json`:

    {"character": ..., "body": ..., "head": ..., "units": "glb",
     "boxes": [{"joint": <rig row>, "bone": "Bone_3", "hitpart": 8,
                "min": [x, y, z], "max": [x, y, z], "bbox_parent": <index>|null,
                "source": "body"|"head"}]}   -- in model_test_for_hit's order

Usage:
    python tools/pd-assets/pd_hitbox.py [roster.json] [outdir]
    (defaults: tools/pd-assets/pd_roster.json, native/assets/enemies/pd/characters)
"""

from __future__ import annotations

import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import pd_gltf  # noqa: E402
from pd_model import load, seg_off, seg_ok  # noqa: E402

REPO = os.path.dirname(os.path.dirname(HERE))
NODE_POSITION = 0x02
NODE_CHRINFO = 0x01
NODE_BBOX = 0x0A
HEADSPOT = 0x17

#: `HITPART_*` (`constants.h:1394`), for the report.
HITPARTS = {
    1: "LFOOT", 2: "LSHIN", 3: "LTHIGH", 4: "RFOOT", 5: "RSHIN", 6: "RTHIGH", 7: "PELVIS",
    8: "HEAD", 9: "LHAND", 10: "LFOREARM", 11: "LBICEP", 12: "RHAND", 13: "RFOREARM",
    14: "RBICEP", 15: "TORSO", 16: "TAIL", 100: "GUN", 110: "HAT", 200: "GENERAL", 201: "GENERALHALF",
}


def boxes_of(model, slot_to_joint, forced_joint, rig, source, head_boxes=None):
    """The model's BBOX nodes in `model_test_for_hit`'s order (`model.c:3785`: a
    depth-first walk that returns the FIRST box the shot passes through, and skips
    a missed box's children), each with the rig joint that poses it. A grafted
    head's boxes (`head_boxes`) go where the body's HEADSPOT node is, as
    `model_attach_head` puts the head's tree there."""
    out = []
    nodes = model.walk()
    by_off = {n.offset: n for n in nodes}

    def bbox_parent(n):
        cur = n
        while seg_ok(cur.parent):
            p = by_off.get(seg_off(cur.parent))
            if p is None:
                return None
            if (p.type & 0xFF) == NODE_BBOX:
                return p.offset
            cur = p
        return None

    index_of = {}
    for n in nodes:
        t = n.type & 0xFF
        if t == HEADSPOT and head_boxes:
            base = len(out)
            for hb in head_boxes:
                hb = dict(hb)
                if hb["bbox_parent"] is not None:
                    hb["bbox_parent"] += base
                out.append(hb)
            continue
        if t != NODE_BBOX or not seg_ok(n.rodata):
            continue
        hitpart, x0, x1, y0, y1, z0, z1 = struct.unpack_from(">iffffff", model.data, seg_off(n.rodata))
        joint = forced_joint
        if joint is None:
            # model_find_node_mtx(model, node, 0): the nearest POSITION (or the
            # root CHRINFO) ancestor's first matrix slot.
            cur = n
            slot = None
            while seg_ok(cur.parent):
                p = by_off.get(seg_off(cur.parent))
                if p is None:
                    break
                pt = p.type & 0xFF
                if pt == NODE_POSITION and seg_ok(p.rodata):
                    slot = struct.unpack_from(">fffHhhh", model.data, seg_off(p.rodata))[4]
                    break
                if pt == NODE_CHRINFO and seg_ok(p.rodata):
                    slot = struct.unpack_from(">Hh", model.data, seg_off(p.rodata))[1]
                    break
                cur = p
            joint = slot_to_joint.get(slot) if slot is not None else None
        if joint is None:
            raise SystemExit(f"{model.name}: BBOX at {n.offset:#x} has no posing joint")
        s = pd_gltf.EXPORT_SCALE
        bp = bbox_parent(n)
        index_of[n.offset] = len(out)
        out.append({
            "joint": joint,
            "bone": rig.joints[joint].name,
            "hitpart": hitpart,
            "hitpart_name": HITPARTS.get(hitpart, str(hitpart)),
            "min": [min(x0, x1) * s, min(y0, y1) * s, min(z0, z1) * s],
            "max": [max(x0, x1) * s, max(y0, y1) * s, max(z0, z1) * s],
            "bbox_parent": index_of.get(bp) if bp is not None else None,
            "source": source,
        })
    return out


def main() -> int:
    roster = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "pd_roster.json")
    outdir = sys.argv[2] if len(sys.argv) > 2 else os.path.join(REPO, "native", "assets", "enemies", "pd", "characters")
    with open(roster, encoding="utf-8") as fh:
        man = json.load(fh)
    chrs = os.path.join(pd_gltf.assets_root(), "files", "chrs")
    for name, src in man.get("characters", {}).items():
        body, head = (src, None) if isinstance(src, str) else (src[0], src[1])
        model = load(os.path.join(chrs, body + ".bin"))
        rig = pd_gltf.Rig(model)
        head_boxes = None
        if head:
            hm = load(os.path.join(chrs, head + ".bin"))
            hj = rig.slot_to_joint.get(pd_gltf.HEAD_MATRIX_SLOT)
            head_boxes = boxes_of(hm, {}, hj, rig, "head")
        boxes = boxes_of(model, rig.slot_to_joint, None, rig, "body", head_boxes)
        doc = {"character": name, "body": body, "head": head, "units": "glb (pd_gltf EXPORT_SCALE)", "boxes": boxes}
        path = os.path.join(outdir, name + ".hitboxes.json")
        with open(path, "w", encoding="utf-8", newline="\n") as fh:
            json.dump(doc, fh, indent=1)
        parts = sorted({b["hitpart_name"] for b in boxes})
        print(f"{name}: {len(boxes)} boxes ({', '.join(parts)}) -> {os.path.relpath(path, REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
