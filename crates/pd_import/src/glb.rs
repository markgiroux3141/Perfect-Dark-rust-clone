//! A glTF 2.0 reader: a binary `.glb` (the JSON chunk, the BIN chunk) or a
//! text `.gltf` (its buffers in files beside it or in `data:` URIs), with
//! float/integer accessors read out of the buffers, the scene graph's world
//! matrices, triangle lists, strips and fans, and embedded or external
//! images. No sparse accessors, no extensions (a file that requires one is
//! refused by name).

use std::path::{Path, PathBuf};

use glam::{Mat4, Quat, Vec3};
use serde_json::Value;

pub struct Glb {
    pub json: Value,
    /// Each `buffers[]` entry's bytes (a `.glb`'s first one is its BIN chunk).
    pub buffers: Vec<Vec<u8>>,
    /// The file's directory: external buffers and images are relative to it.
    dir: PathBuf,
}

/// Extensions a file may *require* that change nothing here.
const HARMLESS_EXTENSIONS: [&str; 1] = ["KHR_materials_unlit"];

impl Glb {
    pub fn load(path: &Path) -> Result<Glb, String> {
        let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize;
        let (json, bin): (Value, Option<Vec<u8>>) = if b.len() >= 4 && &b[0..4] == b"glTF" {
            if b.len() < 20 || u32_at(4) != 2 {
                return Err(format!("{}: not a glTF 2.0 binary", path.display()));
            }
            let jlen = u32_at(12);
            if u32_at(16) != 0x4E4F534A || 20 + jlen > b.len() {
                return Err(format!("{}: the first chunk is not JSON", path.display()));
            }
            let json = serde_json::from_slice(&b[20..20 + jlen]).map_err(|e| format!("{}: {e}", path.display()))?;
            let o = (20 + jlen + 3) & !3;
            let bin = (o + 8 <= b.len() && u32_at(o + 4) == 0x004E4942).then(|| b[o + 8..(o + 8 + u32_at(o)).min(b.len())].to_vec());
            (json, bin)
        } else {
            let json: Value = serde_json::from_slice(&b).map_err(|e| format!("{}: neither a .glb nor .gltf JSON ({e})", path.display()))?;
            (json, None)
        };
        if let Some(req) = json["extensionsRequired"].as_array() {
            let bad: Vec<&str> = req.iter().filter_map(|e| e.as_str()).filter(|e| !HARMLESS_EXTENSIONS.contains(e)).collect();
            if !bad.is_empty() {
                return Err(format!("{}: requires glTF extensions this importer doesn't read: {}", path.display(), bad.join(", ")));
            }
        }
        let mut buffers = Vec::new();
        let mut bin = bin;
        for (i, buf) in json["buffers"].as_array().into_iter().flatten().enumerate() {
            let bytes = match buf["uri"].as_str() {
                Some(uri) => read_uri(&dir, uri).map_err(|e| format!("{}: buffer {i}: {e}", path.display()))?,
                None if i == 0 => bin.take().ok_or_else(|| format!("{}: buffer 0 has no URI and there is no BIN chunk", path.display()))?,
                None => return Err(format!("{}: buffer {i} has no URI", path.display())),
            };
            buffers.push(bytes);
        }
        Ok(Glb { json, buffers, dir })
    }

    pub fn nodes(&self) -> &[Value] {
        self.json["nodes"].as_array().map_or(&[], |v| v.as_slice())
    }

    /// The node named `name`.
    pub fn node(&self, name: &str) -> Option<&Value> {
        self.nodes().iter().find(|n| n["name"] == name)
    }

    pub fn children<'a>(&'a self, node: &'a Value) -> impl Iterator<Item = &'a Value> + 'a {
        node["children"].as_array().into_iter().flatten().filter_map(move |i| i.as_u64().and_then(|i| self.nodes().get(i as usize)))
    }

    /// A node's local matrix: `matrix` (column-major), else `translation` ×
    /// `rotation` × `scale`.
    pub fn local_matrix(node: &Value) -> Mat4 {
        let f = |v: &Value, k: usize, d: f32| v[k].as_f64().map_or(d, |x| x as f32);
        if let Some(m) = node["matrix"].as_array().filter(|m| m.len() == 16) {
            return Mat4::from_cols_array(&std::array::from_fn(|k| m[k].as_f64().unwrap_or(0.0) as f32));
        }
        let t = &node["translation"];
        let r = &node["rotation"];
        let s = &node["scale"];
        Mat4::from_scale_rotation_translation(
            Vec3::new(f(s, 0, 1.0), f(s, 1, 1.0), f(s, 2, 1.0)),
            Quat::from_xyzw(f(r, 0, 0.0), f(r, 1, 0.0), f(r, 2, 0.0), f(r, 3, 1.0)).normalize(),
            Vec3::new(f(t, 0, 0.0), f(t, 1, 0.0), f(t, 2, 0.0)),
        )
    }

    /// Every node of the default scene (else the first, else every root)
    /// with its world matrix, parents first.
    pub fn scene_nodes(&self) -> Vec<(usize, Mat4)> {
        let scenes = self.json["scenes"].as_array();
        let scene = self.json["scene"].as_u64().map(|s| s as usize).unwrap_or(0);
        let roots: Vec<usize> = match scenes.and_then(|s| s.get(scene)) {
            Some(s) => s["nodes"].as_array().into_iter().flatten().filter_map(|n| n.as_u64().map(|n| n as usize)).collect(),
            None => {
                // No scene: every node that is nobody's child.
                let mut child = vec![false; self.nodes().len()];
                for n in self.nodes() {
                    for c in n["children"].as_array().into_iter().flatten().filter_map(|c| c.as_u64()) {
                        if let Some(x) = child.get_mut(c as usize) {
                            *x = true;
                        }
                    }
                }
                (0..child.len()).filter(|&i| !child[i]).collect()
            }
        };
        let mut out = Vec::new();
        let mut stack: Vec<(usize, Mat4)> = roots.into_iter().rev().map(|r| (r, Mat4::IDENTITY)).collect();
        let mut seen = vec![false; self.nodes().len()];
        while let Some((i, parent)) = stack.pop() {
            let Some(node) = self.nodes().get(i) else { continue };
            if std::mem::replace(&mut seen[i], true) {
                continue;
            }
            let m = parent * Self::local_matrix(node);
            out.push((i, m));
            for c in node["children"].as_array().into_iter().flatten().rev().filter_map(|c| c.as_u64()) {
                stack.push((c as usize, m));
            }
        }
        out
    }

    /// A buffer view's bytes.
    pub fn view(&self, index: usize) -> Result<&[u8], String> {
        let v = &self.json["bufferViews"][index];
        let buf = v["buffer"].as_u64().unwrap_or(0) as usize;
        let off = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = v["byteLength"].as_u64().ok_or("a bufferView without a length")? as usize;
        self.buffers.get(buf).and_then(|b| b.get(off..off + len)).ok_or_else(|| format!("bufferView {index} is past its buffer"))
    }

    /// An accessor's elements as floats (normalised integers scaled to 0..1),
    /// `components` per element.
    pub fn floats(&self, accessor: usize) -> Result<(Vec<f32>, usize), String> {
        let a = &self.json["accessors"][accessor];
        let count = a["count"].as_u64().ok_or("an accessor without a count")? as usize;
        let comps = match a["type"].as_str() {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            t => return Err(format!("accessor {accessor}: type {t:?}")),
        };
        if !a["sparse"].is_null() {
            return Err(format!("accessor {accessor}: sparse accessors aren't read"));
        }
        let ctype = a["componentType"].as_u64().unwrap_or(0);
        let size = match ctype {
            5126 | 5125 => 4,
            5123 | 5122 => 2,
            5121 | 5120 => 1,
            t => return Err(format!("accessor {accessor}: component type {t}")),
        };
        // No buffer view: every element zero (glTF 2.0 § 5.1.1).
        let Some(bv) = a["bufferView"].as_u64().map(|b| b as usize) else { return Ok((vec![0.0; count * comps], comps)) };
        let data = self.view(bv)?;
        let base = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = self.json["bufferViews"][bv]["byteStride"].as_u64().map_or(comps * size, |s| s as usize);
        let normalized = a["normalized"].as_bool().unwrap_or(false);
        let mut out = Vec::with_capacity(count * comps);
        for i in 0..count {
            for c in 0..comps {
                let o = base + i * stride + c * size;
                let s = data.get(o..o + size).ok_or_else(|| format!("accessor {accessor} is past its view"))?;
                let v = match ctype {
                    5126 => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                    5125 => u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32,
                    5123 => u16::from_le_bytes([s[0], s[1]]) as f32 / if normalized { 65535.0 } else { 1.0 },
                    5122 => i16::from_le_bytes([s[0], s[1]]) as f32 / if normalized { 32767.0 } else { 1.0 },
                    5121 => s[0] as f32 / if normalized { 255.0 } else { 1.0 },
                    _ => s[0] as i8 as f32 / if normalized { 127.0 } else { 1.0 },
                };
                out.push(v);
            }
        }
        Ok((out, comps))
    }

    /// A primitive's triangles as vertex indices, three per triangle: a
    /// triangle list, strip or fan (its `indices`, or 0..n). `None` for
    /// points and lines, which draw nothing here.
    pub fn triangles(&self, prim: &Value, nverts: usize) -> Result<Option<Vec<usize>>, String> {
        let mode = prim["mode"].as_u64().unwrap_or(4);
        if mode < 4 {
            return Ok(None);
        }
        let idx: Vec<usize> = match prim["indices"].as_u64() {
            Some(a) => self.floats(a as usize)?.0.into_iter().map(|v| v as usize).collect(),
            None => (0..nverts).collect(),
        };
        if let Some(&bad) = idx.iter().find(|&&i| i >= nverts) {
            return Err(format!("an index {bad} past the primitive's {nverts} vertices"));
        }
        Ok(Some(match mode {
            4 => idx[..idx.len() / 3 * 3].to_vec(),
            // A strip: every other triangle flipped to keep the winding.
            5 => (2..idx.len()).flat_map(|k| if k % 2 == 0 { [idx[k - 2], idx[k - 1], idx[k]] } else { [idx[k - 1], idx[k - 2], idx[k]] }).collect(),
            6 => (2..idx.len()).flat_map(|k| [idx[0], idx[k - 1], idx[k]]).collect(),
            m => return Err(format!("primitive mode {m}")),
        }))
    }

    /// A primitive's triangle list as vertex indices (its `indices`, or 0..n).
    pub fn triangle_indices(&self, prim: &Value, nverts: usize) -> Result<Vec<usize>, String> {
        if prim["mode"].as_u64().unwrap_or(4) != 4 {
            return Err("a primitive that is not a triangle list".into());
        }
        Ok(self.triangles(prim, nverts)?.unwrap_or_default())
    }

    /// An image's encoded bytes: a buffer view, a `data:` URI or a file
    /// beside the glTF.
    pub fn image_bytes(&self, image: usize) -> Result<Vec<u8>, String> {
        let im = &self.json["images"][image];
        match (im["bufferView"].as_u64(), im["uri"].as_str()) {
            (Some(bv), _) => Ok(self.view(bv as usize)?.to_vec()),
            (None, Some(uri)) => read_uri(&self.dir, uri).map_err(|e| format!("image {image}: {e}")),
            _ => Err(format!("image {image} has neither a bufferView nor a URI")),
        }
    }

    /// An image's encoded bytes (embedded PNG).
    pub fn image_png(&self, image: usize) -> Result<&[u8], String> {
        let im = &self.json["images"][image];
        if im["mimeType"] != "image/png" {
            return Err(format!("image {image} is not an embedded PNG"));
        }
        self.view(im["bufferView"].as_u64().ok_or("an image without a bufferView")? as usize)
    }
}

/// A URI's bytes: `data:[...];base64,...`, or a file relative to `dir`
/// (percent-escapes undone).
fn read_uri(dir: &Path, uri: &str) -> Result<Vec<u8>, String> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (head, data) = rest.split_once(',').ok_or("a data: URI without a comma")?;
        if !head.ends_with(";base64") {
            return Err("a data: URI that isn't base64".into());
        }
        return base64_decode(data);
    }
    let mut name = Vec::new();
    let b = uri.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&uri[i + 1..i + 3], 16) {
                name.push(v);
                i += 3;
                continue;
            }
        }
        name.push(b[i]);
        i += 1;
    }
    let path = dir.join(String::from_utf8_lossy(&name).as_ref());
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Standard base64 (padding optional, whitespace skipped).
fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        acc = (acc << 6) | val(c).ok_or_else(|| format!("{:?} in base64", c as char))?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_with_and_without_padding() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64_decode("AAEC\n/w==").unwrap(), [0, 1, 2, 255]);
    }
}
