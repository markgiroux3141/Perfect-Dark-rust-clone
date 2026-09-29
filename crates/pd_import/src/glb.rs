//! A minimal glTF 2.0 binary (`.glb`) reader: the JSON chunk, the BIN chunk,
//! and float/integer accessors read out of it. Enough for the scenes the OoT
//! extractor writes (unindexed or indexed triangle lists, embedded PNGs); no
//! sparse accessors, no external buffers.

use serde_json::Value;

pub struct Glb {
    pub json: Value,
    pub bin: Vec<u8>,
}

impl Glb {
    pub fn load(path: &std::path::Path) -> Result<Glb, String> {
        let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize;
        if b.len() < 20 || &b[0..4] != b"glTF" || u32_at(4) != 2 {
            return Err(format!("{}: not a glTF 2.0 binary", path.display()));
        }
        let jlen = u32_at(12);
        if u32_at(16) != 0x4E4F534A {
            return Err(format!("{}: the first chunk is not JSON", path.display()));
        }
        let json: Value = serde_json::from_slice(&b[20..20 + jlen]).map_err(|e| format!("{}: {e}", path.display()))?;
        let o = 20 + jlen;
        let bin = if o + 8 <= b.len() && u32_at(o + 4) == 0x004E4942 { b[o + 8..o + 8 + u32_at(o)].to_vec() } else { Vec::new() };
        Ok(Glb { json, bin })
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

    /// A buffer view's bytes.
    pub fn view(&self, index: usize) -> Result<&[u8], String> {
        let v = &self.json["bufferViews"][index];
        let off = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = v["byteLength"].as_u64().ok_or("a bufferView without a length")? as usize;
        self.bin.get(off..off + len).ok_or_else(|| format!("bufferView {index} is past the BIN chunk"))
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
        let ctype = a["componentType"].as_u64().unwrap_or(0);
        let size = match ctype {
            5126 | 5125 => 4,
            5123 | 5122 => 2,
            5121 | 5120 => 1,
            t => return Err(format!("accessor {accessor}: component type {t}")),
        };
        let bv = a["bufferView"].as_u64().ok_or("a sparse accessor")? as usize;
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

    /// A primitive's triangle list as vertex indices (its `indices`, or 0..n).
    pub fn triangle_indices(&self, prim: &Value, nverts: usize) -> Result<Vec<usize>, String> {
        if prim["mode"].as_u64().unwrap_or(4) != 4 {
            return Err("a primitive that is not a triangle list".into());
        }
        match prim["indices"].as_u64() {
            Some(a) => Ok(self.floats(a as usize)?.0.into_iter().map(|v| v as usize).collect()),
            None => Ok((0..nverts).collect()),
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
