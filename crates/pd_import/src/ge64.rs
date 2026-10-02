//! A converted level as the GoldenEye Setup Editor's level files (SubDrag's
//! editor, for building GoldenEye and Perfect Dark ROM levels), in the very
//! shape it exports a level in: what its "import level from visual" reads.
//!
//! Matched against the editor's own export of Complex (V4.4), whose numbers
//! are ours (`assets/stages/ref`) one for one:
//! - `level/LevelIndices.obj` + `.mtl`: a group a room and layer,
//!   `primary_RoomXX` (opaque) and `secondary_RoomXX` (translucent), the room
//!   in two hex digits; every vertex `v` (world cm, whole numbers: the ROM's
//!   are), `vt` (texels / the texture's size, unflipped), `#vcolor r g b a`
//!   (0-255), `vn 0 0 0`; faces `f i/i/i` then `#fvcolorindex i j k`.
//!   Materials `m<n>` with `ClampS`/`ClampT`/`MirrorS`/`MirrorT`,
//!   `Transparent`, `CullBoth` appended; `map_Kd` a BMP beside it (24-bit,
//!   32-bit with alpha).
//! - `portal/portals.obj` + `.mtl` + `portals.txt`: a group a portal,
//!   `Portal_AAA_BBB vN` (rooms in three hex digits); the text file
//!   `AAA BBB 0000 04` and its four corners.
//! - `clipping/clippingObj<code>.obj` + `.mtl`: a group a room, `ClipXXX`
//!   (three hex digits, from 000), triangles under `m<n>_<tags>_SFX<floortype>`
//!   materials (`ForceFloor`, `ForceWall`, `Railing`, `SolidLadder`,
//!   `TransparentLadder`, `Crouch_`), each corner's `#vcolor` the floor's
//!   tint (`floorcol`'s nibbles × 16).
//!
//! The textures are cut to N64 sizes (a power of two a side, at most
//! `max_texels`), as the console's 4 KB of texture memory needs.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use glam::Vec3;
use pd_core::ids::*;

use crate::rooms::Partition;
use crate::source::*;

pub struct Options {
    pub rooms: usize,
    /// A texture's texels at most (2048: 64 × 32 at 16 bits a texel).
    pub max_texels: u32,
    pub max_side: u32,
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// A BMP: 24-bit, or 32-bit (BGRA) when `alpha`; rows bottom up.
fn bmp(px: &image::RgbaImage, alpha: bool) -> Vec<u8> {
    let (w, h) = (px.width() as usize, px.height() as usize);
    let bpp = if alpha { 4 } else { 3 };
    let row = (w * bpp).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut o = Vec::with_capacity(size);
    o.extend(b"BM");
    o.extend((size as u32).to_le_bytes());
    o.extend([0u8; 4]);
    o.extend(54u32.to_le_bytes());
    o.extend(40u32.to_le_bytes());
    o.extend((w as i32).to_le_bytes());
    o.extend((h as i32).to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend((bpp as u16 * 8).to_le_bytes());
    o.extend(0u32.to_le_bytes()); // BI_RGB
    o.extend(((row * h) as u32).to_le_bytes());
    o.extend(2835u32.to_le_bytes());
    o.extend(2835u32.to_le_bytes());
    o.extend([0u8; 8]);
    for y in (0..h).rev() {
        let start = o.len();
        for x in 0..w {
            let p = px.get_pixel(x as u32, y as u32).0;
            o.extend([p[2], p[1], p[0]]);
            if alpha {
                o.push(p[3]);
            }
        }
        o.resize(start + row, 0);
    }
    o
}

/// The N64 size for a `w` × `h` texture: powers of two keeping its shape, no
/// side over `max_side`, no more than `max_texels`.
pub fn n64_size(w: u32, h: u32, max_side: u32, max_texels: u32) -> (u32, u32) {
    let (mut tw, mut th) = (w.max(1).next_power_of_two(), h.max(1).next_power_of_two());
    // Halve both sides while either can be (the shape kept), then the longer.
    while tw.max(th) > max_side.max(1) || tw * th > max_texels.max(1) {
        if tw > 1 && th > 1 {
            tw /= 2;
            th /= 2;
        } else if tw > th {
            tw /= 2;
        } else {
            th /= 2;
        }
    }
    (tw, th)
}

/// A clipping triangle's material tags, as the editor names PD's tile flags.
fn clip_tags(flags: u32) -> &'static str {
    let floor = flags & (GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2) != 0;
    let solid = flags & (GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT) != 0;
    // A ladder only players climb (PD's GEOFLAG_LADDER_PLAYERONLY: our
    // ledges) has no tag; it goes as a ladder.
    let ladder = flags & (GEOFLAG_LADDER | GEOFLAG_LADDER_PLAYERONLY) != 0;
    match (floor, ladder, solid) {
        (true, _, _) if flags & GEOFLAG_AIBOTCROUCH != 0 => "Crouch_ForceFloor",
        (true, _, _) => "ForceFloor",
        (false, true, true) => "SolidLadder",
        (false, true, false) => "TransparentLadder",
        (false, false, true) => "ForceWall",
        (false, false, false) => "Railing",
    }
}

/// Write `src`, split into `opts.rooms` rooms, as the editor's level files
/// under `out`. Returns the report.
pub fn export(code: &str, src: &LevelSource, opts: &Options, out: &Path) -> Result<Vec<String>, String> {
    let part = Partition::build(src, opts.rooms);
    let mut report = Vec::new();
    if out.exists() {
        for sub in ["level", "portal", "clipping"] {
            let d = out.join(sub);
            if d.exists() {
                std::fs::remove_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
            }
        }
    }
    let header = "# Created by pd_import (Perfect Dark Rust clone), in the GoldenEye Setup Editor V4.4's export format\n";

    // ── The textures and materials ─────────────────────────────────────────
    let mut mtl = String::new();
    let mut bmp_of: HashMap<usize, String> = HashMap::new();
    let mut resized = Vec::new();
    for (i, t) in src.textures.iter().enumerate() {
        let img = image::load_from_memory_with_format(&t.png, image::ImageFormat::Png).map_err(|e| format!("texture {i}: {e}"))?.to_rgba8();
        let (w, h) = n64_size(img.width(), img.height(), opts.max_side, opts.max_texels);
        let px = if (w, h) == img.dimensions() { img } else { image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle) };
        let alpha = px.pixels().any(|p| p.0[3] < 255);
        let name = format!("{code}_{i:03}.bmp");
        std::fs::create_dir_all(out.join("level")).map_err(|e| e.to_string())?;
        std::fs::write(out.join("level").join(&name), bmp(&px, alpha)).map_err(|e| e.to_string())?;
        resized.push(format!("{w}x{h}"));
        bmp_of.insert(i, name);
    }
    let mut mat_name = Vec::new();
    for (i, m) in src.materials.iter().enumerate() {
        let mut n = format!("m{i}");
        if m.blend == Blend::Translucent {
            n += "Transparent";
        }
        if !m.cull_back {
            n += "CullBoth";
        }
        for (axis, w) in ["S", "T"].iter().zip(m.wrap) {
            match w {
                Wrap::Clamp => n += &format!("Clamp{axis}"),
                Wrap::Mirror => n += &format!("Mirror{axis}"),
                Wrap::Repeat => {}
            }
        }
        let _ = writeln!(mtl, "newmtl {n}\nKd 1.0 1.0 1.0\nKa 0 0 0\nillum 2\nNs 64\nd 1.000000");
        if let Some(b) = m.texture.and_then(|t| bmp_of.get(&t)) {
            let _ = writeln!(mtl, "map_Kd {b}");
        }
        mtl.push('\n');
        mat_name.push(n);
    }
    let mut sizes: HashMap<&str, usize> = HashMap::new();
    for s in &resized {
        *sizes.entry(s.as_str()).or_default() += 1;
    }
    let mut sizes: Vec<_> = sizes.into_iter().collect();
    sizes.sort();
    report.push(format!("textures: {} BMPs ({})", resized.len(), sizes.iter().map(|(s, n)| format!("{n} at {s}")).collect::<Vec<_>>().join(", ")));

    // ── The visual: every triangle cut into its rooms ──────────────────────
    // (room, translucent) -> material -> triangles.
    type ByMaterial = HashMap<usize, Vec<[Vert; 3]>>;
    let mut groups: HashMap<(u16, bool), ByMaterial> = HashMap::new();
    for t in &src.tris {
        for (room, piece) in part.cut(t.v.to_vec()) {
            let g = groups.entry((room, t.xlu)).or_default().entry(t.material).or_default();
            for k in 1..piece.len() - 1 {
                g.push([piece[0], piece[k], piece[k + 1]]);
            }
        }
    }
    let (mut verts, mut faces) = (String::new(), String::new());
    let mut nv = 0usize;
    let mut room_tris: HashMap<u16, usize> = HashMap::new();
    let mut max_uv: f32 = 0.0;
    for xlu in [false, true] {
        for room in 1..=part.rooms() as u16 {
            let Some(mats) = groups.get(&(room, xlu)) else { continue };
            let _ = writeln!(faces, "g {}_Room{room:02X}", if xlu { "secondary" } else { "primary" });
            let mut ms: Vec<_> = mats.keys().copied().collect();
            ms.sort();
            for m in ms {
                let _ = writeln!(faces, "usemtl {}", mat_name[m]);
                let wrap = src.materials[m].wrap;
                for tri in &mats[&m] {
                    // Each triangle's texture coordinates moved by whole
                    // repeats (two, mirrored) to start near 0: the N64 holds
                    // them in ±1024 texels.
                    let shift: [f32; 2] = std::array::from_fn(|k| {
                        let lo = tri.iter().map(|v| v.uv[k]).fold(f32::INFINITY, f32::min);
                        match wrap[k] {
                            Wrap::Repeat => lo.floor(),
                            Wrap::Mirror => (lo / 2.0).floor() * 2.0,
                            Wrap::Clamp => 0.0,
                        }
                    });
                    for v in tri {
                        let (u, t) = (v.uv.x - shift[0], v.uv.y - shift[1]);
                        max_uv = max_uv.max(u.abs()).max(t.abs());
                        let p = v.pos.round();
                        let _ = writeln!(verts, "v {:.6} {:.6} {:.6}\nvt {u:.6} {t:.6} 0.0\n#vcolor {:.6} {:.6} {:.6} {:.6}\nvn 0 0 0", p.x, p.y, p.z, v.col[0] as f32, v.col[1] as f32, v.col[2] as f32, v.col[3] as f32);
                    }
                    let (a, b, c) = (nv + 1, nv + 2, nv + 3);
                    nv += 3;
                    let _ = writeln!(faces, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}\n#fvcolorindex {a} {b} {c} ");
                    *room_tris.entry(room).or_default() += 1;
                }
            }
        }
    }
    write(&out.join("level").join("LevelIndices.obj"), &format!("{header}mtllib LevelIndices.mtl\n{verts}{faces}"))?;
    write(&out.join("level").join("LevelIndices.mtl"), &mtl)?;
    let total: usize = room_tris.values().sum();
    let busiest = room_tris.iter().max_by_key(|(_, n)| **n).map(|(r, n)| (*r, *n)).unwrap_or((0, 0));
    report.push(format!(
        "level: {} rooms, {total} triangles (the busiest room {:02X}: {}), {} materials; texture coordinates within ±{:.1} repeats",
        part.rooms(),
        busiest.0,
        busiest.1,
        src.materials.len(),
        max_uv
    ));

    // ── The portals ────────────────────────────────────────────────────────
    let portals = part.portals();
    let (mut pobj, mut ptxt) = (format!("{header}mtllib portals.mtl\n"), String::new());
    for (i, p) in portals.iter().enumerate() {
        let [a, b] = p.rooms;
        let _ = writeln!(pobj, "g Portal_{a:03X}_{b:03X} v{}", i * 4 + 1);
        let _ = writeln!(ptxt, "{a:03X} {b:03X} 0000 04");
        for v in &p.verts {
            let v = v.round();
            let _ = writeln!(pobj, "v {:.6} {:.6} {:.6}", v.x, v.y, v.z);
            let _ = writeln!(ptxt, "{:.10} {:.10} {:.10}", v.x, v.y, v.z);
        }
        let _ = writeln!(pobj, "f {} {} {} {}", i * 4 + 1, i * 4 + 2, i * 4 + 3, i * 4 + 4);
    }
    write(&out.join("portal").join("portals.obj"), &pobj)?;
    write(&out.join("portal").join("portals.mtl"), "")?;
    write(&out.join("portal").join("portals.txt"), &ptxt)?;
    report.push(format!("portals: {}", portals.len()));

    // ── The clipping: every tile cut into its rooms ────────────────────────
    let mut by_room: HashMap<u16, Vec<(&ColPoly, Vec<Vec3>)>> = HashMap::new();
    for p in &src.collision {
        for (room, piece) in part.cut(p.verts.clone()) {
            by_room.entry(room).or_default().push((p, piece));
        }
    }
    let mut cmats: Vec<String> = Vec::new();
    let mut cmat_of: HashMap<String, usize> = HashMap::new();
    let mut cobj = format!("{header}mtllib clippingObj{code}.mtl\n");
    let (mut nc, mut ntris) = (0usize, 0usize);
    let mut tag_count: HashMap<String, usize> = HashMap::new();
    for room in 0..=part.rooms() as u16 {
        let _ = writeln!(cobj, "g Clip{room:03X}");
        for (p, piece) in by_room.get(&room).into_iter().flatten() {
            let tag = format!("{}_SFX{}", clip_tags(p.flags), p.floortype);
            let mi = *cmat_of.entry(tag.clone()).or_insert_with(|| {
                cmats.push(format!("m{}_{tag}", cmats.len()));
                cmats.len() - 1
            });
            let c = [(p.floorcol >> 8) & 15, (p.floorcol >> 4) & 15, p.floorcol & 15].map(|n| (n * 16) as f32);
            for k in 1..piece.len().saturating_sub(1) {
                let _ = writeln!(cobj, "usemtl {}", cmats[mi]);
                for v in [piece[0], piece[k], piece[k + 1]] {
                    let v = v.round();
                    let _ = writeln!(cobj, "v {:.6} {:.6} {:.6}", v.x, v.y, v.z);
                }
                let _ = writeln!(cobj, "f {} {} {}", nc + 1, nc + 2, nc + 3);
                for _ in 0..3 {
                    let _ = writeln!(cobj, "#vcolor {:.6} {:.6} {:.6}", c[0], c[1], c[2]);
                }
                let _ = writeln!(cobj, "#fvcolorindex {} {} {}", nc + 1, nc + 2, nc + 3);
                nc += 3;
                ntris += 1;
                *tag_count.entry(tag.clone()).or_default() += 1;
            }
        }
    }
    let cmtl: String = cmats.iter().map(|m| format!("newmtl {m}\nKd 1 1 1\nillum 2\n\n")).collect();
    write(&out.join("clipping").join(format!("clippingObj{code}.obj")), &cobj)?;
    write(&out.join("clipping").join(format!("clippingObj{code}.mtl")), &cmtl)?;
    let mut tags: Vec<_> = tag_count.into_iter().collect();
    tags.sort_by(|a, b| b.1.cmp(&a.1));
    report.push(format!("clipping: {ntris} triangles ({})", tags.iter().map(|(t, n)| format!("{n} {t}")).collect::<Vec<_>>().join(", ")));

    // ── The source's own spots, for placing the setup by hand ──────────────
    let mut mk = String::from("# The source level's own spots, in the editor's units (cm): kind, x y z (the floor), facing in degrees (0 = +z), room
");
    for m in &src.markers {
        let kind = if m.kind == MarkerKind::Item { "weapon" } else { m.kind.name() };
        let _ = writeln!(mk, "{kind} {:.0} {:.0} {:.0} {:.0} {:02X}", m.pos.x, m.pos.y, m.pos.z, m.facing.to_degrees(), part.room_at(m.pos));
    }
    write(&out.join("markers.txt"), &mk)?;
    report.push(format!("markers: {} (markers.txt)", src.markers.len()));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_fit_texture_memory() {
        assert_eq!(n64_size(512, 512, 64, 2048), (32, 32));
        assert_eq!(n64_size(256, 512, 64, 2048), (32, 64));
        assert_eq!(n64_size(1024, 128, 64, 2048), (64, 8));
        assert_eq!(n64_size(64, 64, 64, 2048), (32, 32), "square stays square");
        assert_eq!(n64_size(48, 30, 64, 2048), (64, 32));
        assert_eq!(n64_size(8, 8, 64, 2048), (8, 8));
    }

    #[test]
    fn clipping_tags_are_the_editors() {
        // Complex's own: 0x1b floors, 0x1c walls, 0x04 railings, 0x44
        // see-through ladders, 0x81b crouch floors.
        assert_eq!(clip_tags(0x1b), "ForceFloor");
        assert_eq!(clip_tags(0x1c), "ForceWall");
        assert_eq!(clip_tags(0x04), "Railing");
        assert_eq!(clip_tags(0x44), "TransparentLadder");
        assert_eq!(clip_tags(0x5c), "SolidLadder");
        assert_eq!(clip_tags(0x81b), "Crouch_ForceFloor");
        assert_eq!(clip_tags(0x801c), "SolidLadder");
    }

    #[test]
    fn bmps_are_bottom_up_and_padded() {
        let px = image::RgbaImage::from_fn(3, 2, |x, y| image::Rgba([x as u8, y as u8, 9, 255]));
        let b = bmp(&px, false);
        assert_eq!(&b[0..2], b"BM");
        assert_eq!(b.len(), 54 + 12 * 2, "3 × 3 bytes padded to 12 a row");
        // The first row stored is the image's last (y = 1): BGR.
        assert_eq!(&b[54..57], &[9, 1, 0]);
        assert_eq!(bmp(&px, true).len(), 54 + 12 * 2);
    }
}
