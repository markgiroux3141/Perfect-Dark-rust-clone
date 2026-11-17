//! The GE64 OBJ writer: `LevelIndices.obj`, its `.mtl`, `_texturePresets.txt` and the textures
//! beside it, as `blender_io.export_level` writes them from Blender.
//!
//! The dialect (`formats/ge64_obj.py`, `formats/level_files.py`): standard OBJ plus `#vcolor r g
//! b` (0-255) and `#fvcolorindex i j k` after each `f`. Each room is two groups, `primary_RoomNN`
//! and then `secondary_RoomNN` (its Transparent faces), in the order of the rooms' numbers. A
//! group lists its `#vcolor`s, its `v`s, its `vt`s and its `vn`s, then its faces; a `vt` and its
//! `#vcolor` share a number, one for each different (uv, colour) corner. Positions are GE's
//! (x, z, -y) of the Blender-space mesh, whole numbers. Materials are named `m<i><Flags>` in
//! the order they are first used.
//!
//! Textures are ROM data: none is in this crate. They are copied from a folder the caller
//! names (`tempImgEd<preset>.bmp`); a missing one is a warning, and the level is still written.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use autolevel::py;

use crate::complex as cx;
use crate::mesh::Room;

/// The editor's order for a material's flags in its name.
pub const FLAG_TOKENS: [&str; 10] = ["TopFlag", "EnvMapping", "Transparent", "CullBoth", "CullBack", "CullFront", "ClampS", "MirrorS", "ClampT", "MirrorT"];

/// What was written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub rooms: usize,
    pub triangles: usize,
    /// The materials' names, in order, each with its place in `complex::PALETTE`.
    pub materials: Vec<(String, usize)>,
    pub warnings: Vec<String>,
}

/// `m<index><Flags>`, the flags in the editor's order (`level_files.material_name`).
pub fn material_name(index: usize, flags: &[&str]) -> String {
    let mut out = format!("m{index}");
    for t in FLAG_TOKENS {
        if flags.contains(&t) {
            out.push_str(t);
        }
    }
    out
}

/// The file a preset's texture is in.
pub fn texture_file(preset: &str) -> String {
    format!("tempImgEd{preset}.bmp")
}

/// One corner of a triangle: a position (a place in the group's own list), and its uv and colour.
struct Corner {
    v: usize,
    uv: (f64, f64),
    color: [u8; 3],
}

struct Tri {
    corners: [Corner; 3],
    normal: [f64; 3],
    material: usize,
}

/// Blender's (x, y, z) as GE's (x, z, -y).
fn ge(p: [i32; 3]) -> [i32; 3] {
    [p[0], p[2], -p[1]]
}

/// The unit normal of a triangle of GE points (zeros if it has no area).
fn normal(a: [i32; 3], b: [i32; 3], c: [i32; 3]) -> [f64; 3] {
    let u = [(b[0] - a[0]) as f64, (b[1] - a[1]) as f64, (b[2] - a[2]) as f64];
    let w = [(c[0] - a[0]) as f64, (c[1] - a[1]) as f64, (c[2] - a[2]) as f64];
    let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len == 0.0 {
        [0.0; 3]
    } else {
        [n[0] / len + 0.0, n[1] / len + 0.0, n[2] / len + 0.0]
    }
}

/// The OBJ's text and what it holds. `rooms` in any order; they are written by number.
pub fn text(rooms: &[Room], header: &str) -> (String, Report) {
    let mut order: Vec<&Room> = rooms.iter().collect();
    order.sort_by_key(|r| r.number);
    let mut report = Report { rooms: order.len(), ..Report::default() };
    let mut names: Vec<Option<String>> = vec![None; cx::PALETTE.len()];
    let mut out = String::new();
    let _ = writeln!(out, "# {header}");
    let _ = writeln!(out, "mtllib LevelIndices.mtl");
    let (mut v_base, mut vt_base, mut vn_base) = (0usize, 0usize, 0usize);
    for room in order {
        for v in &room.verts {
            if v.iter().any(|c| c.abs() > 32767) {
                report.warnings.push(format!("{}: vertex {:?} exceeds s16 range", room.name, ge(*v)));
            }
        }
        // the room's triangles in face order, the Transparent ones apart: a material is named when first met
        let (mut primary, mut secondary) = (Vec::new(), Vec::new());
        for f in &room.faces {
            let paint = &cx::PALETTE[f.material];
            if names[f.material].is_none() {
                let name = material_name(report.materials.len(), paint.flags);
                report.materials.push((name.clone(), f.material));
                names[f.material] = Some(name);
            }
            for t in &f.tris {
                let at = |k: usize| f.verts[t[k] as usize] as usize;
                let corner = |k: usize| Corner { v: at(k), uv: f.uvs[t[k] as usize], color: f.colors[t[k] as usize] };
                let n = normal(ge(room.verts[at(0)]), ge(room.verts[at(1)]), ge(room.verts[at(2)]));
                let tri = Tri { corners: [corner(0), corner(1), corner(2)], normal: n, material: f.material };
                if paint.flags.contains(&"Transparent") { &mut secondary } else { &mut primary }.push(tri);
                report.triangles += 1;
            }
        }
        for (layer, tris) in [("primary", primary), ("secondary", secondary)] {
            if tris.is_empty() {
                continue;
            }
            // the group's own numbering: positions as first used, one vt and #vcolor for each different corner
            let mut used_v: Vec<usize> = Vec::new();
            let mut local_v: HashMap<usize, usize> = HashMap::new();
            let mut corners: Vec<((f64, f64), [u8; 3])> = Vec::new();
            let mut local_c: HashMap<(u64, u64, [u8; 3]), usize> = HashMap::new();
            let mut refs: Vec<[(usize, usize); 3]> = Vec::with_capacity(tris.len());
            for t in &tris {
                let mut r = [(0, 0); 3];
                for (k, c) in t.corners.iter().enumerate() {
                    let lv = *local_v.entry(c.v).or_insert_with(|| {
                        used_v.push(c.v);
                        used_v.len() - 1
                    });
                    let uv = (py::round_to(c.uv.0, 6), py::round_to(c.uv.1, 6));
                    // -0.0 and 0.0 are one key, as they are in the Python's dict; the first met is written
                    let lc = *local_c.entry(((uv.0 + 0.0).to_bits(), (uv.1 + 0.0).to_bits(), c.color)).or_insert_with(|| {
                        corners.push((uv, c.color));
                        corners.len() - 1
                    });
                    r[k] = (lv, lc);
                }
                refs.push(r);
            }
            let _ = writeln!(out, "o {layer}_Room{:02X}", room.number);
            for (_, c) in &corners {
                let _ = writeln!(out, "#vcolor {:.6} {:.6} {:.6}", c[0] as f64, c[1] as f64, c[2] as f64);
            }
            for &v in &used_v {
                let p = ge(room.verts[v]);
                let _ = writeln!(out, "v {:.6} {:.6} {:.6}", p[0] as f64, p[1] as f64, p[2] as f64);
            }
            for (uv, _) in &corners {
                let _ = writeln!(out, "vt {:.6} {:.6}", uv.0, uv.1);
            }
            for t in &tris {
                let _ = writeln!(out, "vn {:.4} {:.4} {:.4}", t.normal[0], t.normal[1], t.normal[2]);
            }
            let mut current = usize::MAX;
            for (i, (t, r)) in tris.iter().zip(&refs).enumerate() {
                if t.material != current {
                    current = t.material;
                    let _ = writeln!(out, "usemtl {}", names[current].as_deref().unwrap_or(""));
                }
                let f: Vec<String> = r.iter().map(|(lv, lc)| format!("{}/{}/{}", v_base + lv + 1, vt_base + lc + 1, vn_base + i + 1)).collect();
                let c: Vec<String> = r.iter().map(|(_, lc)| (vt_base + lc + 1).to_string()).collect();
                let _ = writeln!(out, "f {}", f.join(" "));
                let _ = writeln!(out, "#fvcolorindex {}", c.join(" "));
            }
            v_base += used_v.len();
            vt_base += corners.len();
            vn_base += tris.len();
        }
    }
    (out, report)
}

/// The `.mtl`: every material white, with its texture's file name (`level_files.write_mtl`).
pub fn mtl(materials: &[(String, usize)]) -> String {
    let mut out = vec!["# GE64 MTL written by autolevel_build".to_string(), format!("# Material Count: {}", materials.len()), String::new()];
    for (name, m) in materials {
        out.extend([format!("newmtl {name}"), "Ns 64".into(), "Ka 0.000000 0.000000 0.000000".into(), "Kd 1.000000 1.000000 1.000000".into()]);
        out.extend(["d 1.000000".into(), "illum 2".into(), format!("map_Kd {}", texture_file(cx::PALETTE[*m].preset)), String::new()]);
    }
    out.join("\n")
}

/// `_texturePresets.txt`: `<preset> <texture file>` for each texture, once, in the materials' order.
pub fn presets(materials: &[(String, usize)]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = String::new();
    for (_, m) in materials {
        let preset = cx::PALETTE[*m].preset;
        if !seen.contains(&preset) {
            seen.push(preset);
            let _ = writeln!(out, "{preset} {}", texture_file(preset));
        }
    }
    out
}

/// Write `<path>` (the .obj), its `.mtl` and `_texturePresets.txt` beside it, and copy the
/// textures it names from `textures` (when given) into its folder.
pub fn write(path: &Path, rooms: &[Room], textures: Option<&Path>) -> Result<Report, String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| format!("no file name in {}", path.display()))?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (obj, mut report) = text(rooms, "GE64 room geometry written by autolevel_build");
    let obj = obj.replace("mtllib LevelIndices.mtl", &format!("mtllib {stem}.mtl"));
    let put = |name: String, body: &str| fs::write(dir.join(&name), body).map_err(|e| format!("{name}: {e}"));
    put(format!("{stem}.obj"), &obj)?;
    put(format!("{stem}.mtl"), &mtl(&report.materials))?;
    put(format!("{stem}_texturePresets.txt"), &presets(&report.materials))?;
    if let Some(from) = textures {
        let mut done: Vec<String> = Vec::new();
        for (_, m) in &report.materials {
            let file = texture_file(cx::PALETTE[*m].preset);
            if done.contains(&file) {
                continue;
            }
            let (src, dst) = (from.join(&file), dir.join(&file));
            if !src.is_file() {
                report.warnings.push(format!("no texture {}", src.display()));
            } else if src != dst {
                fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
            }
            done.push(file);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Face;

    fn room(number: usize, material: usize) -> Room {
        // one square floor, cut on its diagonal
        let verts = vec![[0, 0, 0], [118, 0, 0], [118, 118, 0], [0, 118, 0]];
        let face = Face {
            verts: vec![0, 1, 2, 3],
            uvs: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            normal: [0.0, 0.0, 1.0],
            material,
            lit: None,
            colors: vec![[167, 167, 167], [255, 255, 255], [167, 167, 167], [90, 90, 90]],
            tris: vec![[0, 1, 2], [0, 2, 3]],
        };
        Room { key: format!("r{number}"), name: format!("Room_{number:02X}"), number, verts, faces: vec![face] }
    }

    #[test]
    fn names_follow_the_editors_order() {
        assert_eq!(material_name(0, &[]), "m0");
        assert_eq!(material_name(13, &["ClampT", "Transparent", "CullBoth"]), "m13TransparentCullBothClampT");
    }

    #[test]
    fn a_room_is_written_as_the_python_writes_it() {
        let floor = cx::PALETTE.iter().position(|p| p.role == "floor").unwrap();
        let (obj, report) = text(&[room(2, floor)], "test");
        let want = "# test\nmtllib LevelIndices.mtl\no primary_Room02\n\
            #vcolor 167.000000 167.000000 167.000000\n#vcolor 255.000000 255.000000 255.000000\n#vcolor 167.000000 167.000000 167.000000\n#vcolor 90.000000 90.000000 90.000000\n\
            v 0.000000 0.000000 0.000000\nv 118.000000 0.000000 0.000000\nv 118.000000 0.000000 -118.000000\nv 0.000000 0.000000 -118.000000\n\
            vt 0.000000 0.000000\nvt 1.000000 0.000000\nvt 1.000000 1.000000\nvt 0.000000 1.000000\n\
            vn 0.0000 1.0000 0.0000\nvn 0.0000 1.0000 0.0000\n\
            usemtl m0\nf 1/1/1 2/2/1 3/3/1\n#fvcolorindex 1 2 3\nf 1/1/2 3/3/2 4/4/2\n#fvcolorindex 1 3 4\n";
        assert_eq!(obj, want);
        assert_eq!(report.triangles, 2);
        assert_eq!(report.materials, vec![("m0".to_string(), floor)]);
    }

    #[test]
    fn rooms_go_by_number_and_glass_goes_second() {
        let floor = cx::PALETTE.iter().position(|p| p.role == "floor").unwrap();
        let glass = cx::PALETTE.iter().position(|p| p.role == "window").unwrap();
        let mut both = room(1, floor);
        let mut pane = both.faces[0].clone();
        pane.material = glass;
        both.faces.insert(0, pane);
        let (obj, report) = text(&[room(3, floor), both], "test");
        let groups: Vec<&str> = obj.lines().filter(|l| l.starts_with("o ")).collect();
        assert_eq!(groups, ["o primary_Room01", "o secondary_Room01", "o primary_Room03"]);
        // the glass was met first, so it is m0, with its flags in the editor's order
        assert_eq!(report.materials[0].0, "m0TransparentCullBothClampT");
        assert_eq!(report.materials[1].0, "m1");
        assert!(presets(&report.materials).starts_with("0190 tempImgEd0190.bmp\n073E tempImgEd073E.bmp\n"));
        assert!(mtl(&report.materials)
            .contains("newmtl m1\nNs 64\nKa 0.000000 0.000000 0.000000\nKd 1.000000 1.000000 1.000000\nd 1.000000\nillum 2\nmap_Kd tempImgEd073E.bmp\n"));
    }
}
