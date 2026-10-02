//! A Quake 3 engine game directory as one file tree: its packages (`.pk3`,
//! zip archives, stored or deflated) and its loose files. Names are matched
//! without case, `/`-separated. A later package (by name) wins over an
//! earlier one, as the engine mounts them (`assets1.pk3` over `assets0.pk3`),
//! and a loose file wins over both (ours: so a file dropped beside the
//! packages replaces theirs).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

struct Entry {
    pk3: usize,
    method: u16,
    csize: u64,
    local: u64,
}

pub struct Vfs {
    dir: PathBuf,
    pk3s: Vec<PathBuf>,
    /// Lower-case name → its entry in the package that wins.
    index: HashMap<String, Entry>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Every entry of the zip at `path`: (name, method, compressed size, local
/// header offset).
fn central_directory(path: &Path) -> Result<Vec<(String, u16, u64, u64)>, String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut f = File::open(path).map_err(err)?;
    let len = f.metadata().map_err(err)?.len();
    // The end record: 22 bytes and a comment of up to 64 KiB.
    let tail = len.min(22 + 0xffff);
    f.seek(SeekFrom::Start(len - tail)).map_err(err)?;
    let mut buf = vec![0; tail as usize];
    f.read_exact(&mut buf).map_err(err)?;
    let eocd = (0..buf.len().saturating_sub(21)).rev().find(|&i| u32_at(&buf, i) == 0x0605_4b50).ok_or_else(|| format!("{}: not a zip (no end record)", path.display()))?;
    let count = u16_at(&buf, eocd + 10) as usize;
    let size = u32_at(&buf, eocd + 12) as u64;
    let offset = u32_at(&buf, eocd + 16) as u64;
    if count == 0xffff || offset == 0xffff_ffff {
        return Err(format!("{}: a zip64 archive (not read)", path.display()));
    }
    f.seek(SeekFrom::Start(offset)).map_err(err)?;
    let mut cd = vec![0; size as usize];
    f.read_exact(&mut cd).map_err(err)?;
    let mut out = Vec::with_capacity(count);
    let mut o = 0;
    for _ in 0..count {
        if o + 46 > cd.len() || u32_at(&cd, o) != 0x0201_4b50 {
            return Err(format!("{}: a bad central directory entry at {o}", path.display()));
        }
        let method = u16_at(&cd, o + 10);
        let csize = u32_at(&cd, o + 20) as u64;
        let (nlen, elen, clen) = (u16_at(&cd, o + 28) as usize, u16_at(&cd, o + 30) as usize, u16_at(&cd, o + 32) as usize);
        let local = u32_at(&cd, o + 42) as u64;
        let name = String::from_utf8_lossy(&cd[o + 46..o + 46 + nlen]).replace('\\', "/");
        out.push((name, method, csize, local));
        o += 46 + nlen + elen + clen;
    }
    Ok(out)
}

impl Vfs {
    /// The game directory `dir` (Jedi Academy's `GameData/base`): every
    /// `*.pk3` in it, and its loose files.
    pub fn open(dir: &Path) -> Result<Vfs, String> {
        let mut pk3s: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pk3")))
            .collect();
        pk3s.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
        let mut index = HashMap::new();
        for (i, p) in pk3s.iter().enumerate() {
            for (name, method, csize, local) in central_directory(p)? {
                if !name.ends_with('/') {
                    index.insert(name.to_ascii_lowercase(), Entry { pk3: i, method, csize, local });
                }
            }
        }
        Ok(Vfs { dir: dir.to_path_buf(), pk3s, index })
    }

    pub fn pk3_count(&self) -> usize {
        self.pk3s.len()
    }

    /// Every packaged file's (lower-case) name.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.index.keys().map(String::as_str)
    }

    /// Is there a file `name`?
    pub fn exists(&self, name: &str) -> bool {
        let name = name.replace('\\', "/").to_ascii_lowercase();
        self.dir.join(&name).is_file() || self.index.contains_key(&name)
    }

    /// The file `name`, if there is one.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let name = name.replace('\\', "/").to_ascii_lowercase();
        let loose = self.dir.join(&name);
        if loose.is_file() {
            return std::fs::read(&loose).map(Some).map_err(|e| format!("{}: {e}", loose.display()));
        }
        let Some(e) = self.index.get(&name) else { return Ok(None) };
        let path = &self.pk3s[e.pk3];
        let err = |x: std::io::Error| format!("{}: {name}: {x}", path.display());
        let mut f = File::open(path).map_err(err)?;
        f.seek(SeekFrom::Start(e.local)).map_err(err)?;
        let mut head = [0u8; 30];
        f.read_exact(&mut head).map_err(err)?;
        if u32_at(&head, 0) != 0x0403_4b50 {
            return Err(format!("{}: {name}: a bad local header", path.display()));
        }
        let skip = u16_at(&head, 26) as i64 + u16_at(&head, 28) as i64;
        f.seek(SeekFrom::Current(skip)).map_err(err)?;
        let mut data = vec![0; e.csize as usize];
        f.read_exact(&mut data).map_err(err)?;
        match e.method {
            0 => Ok(Some(data)),
            8 => miniz_oxide::inflate::decompress_to_vec(&data).map(Some).map_err(|x| format!("{}: {name}: inflate: {x:?}", path.display())),
            m => Err(format!("{}: {name}: compression method {m} (not read)", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip of `(name, data, deflated)` entries, written as an archiver does.
    pub fn zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        for (name, data, deflate) in files {
            let body = if *deflate { miniz_oxide::deflate::compress_to_vec(data, 6) } else { data.to_vec() };
            let method: u16 = if *deflate { 8 } else { 0 };
            let local = out.len() as u32;
            let head = |sig: u32, central: bool| {
                let mut h = Vec::new();
                h.extend(sig.to_le_bytes());
                if central {
                    h.extend(20u16.to_le_bytes());
                }
                h.extend(20u16.to_le_bytes());
                h.extend(0u16.to_le_bytes());
                h.extend(method.to_le_bytes());
                h.extend([0u8; 8]); // time, date, crc (not checked)
                h.extend((body.len() as u32).to_le_bytes());
                h.extend((data.len() as u32).to_le_bytes());
                h.extend((name.len() as u16).to_le_bytes());
                h.extend(0u16.to_le_bytes());
                if central {
                    h.extend([0u8; 6]); // comment length, disk, internal attributes
                    h.extend(0u32.to_le_bytes());
                    h.extend(local.to_le_bytes());
                }
                h.extend(name.as_bytes());
                h
            };
            out.extend(head(0x0403_4b50, false));
            out.extend(&body);
            cd.extend(head(0x0201_4b50, true));
        }
        let at = out.len() as u32;
        out.extend(&cd);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((cd.len() as u32).to_le_bytes());
        out.extend(at.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    #[test]
    fn packages_and_loose_files_layer() {
        let dir = std::env::temp_dir().join(format!("pd_import_pk3_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        let text = b"textures/a/b\n{\n  map textures/a/b\n}\n".repeat(20);
        std::fs::write(dir.join("assets0.pk3"), zip(&[("Shaders/A.shader", &text, true), ("textures/a/b.jpg", b"zero", false), ("scripts/x.txt", b"zero", false)])).unwrap();
        std::fs::write(dir.join("assets1.pk3"), zip(&[("textures/a/b.jpg", b"one", true)])).unwrap();
        std::fs::write(dir.join("scripts/x.txt"), b"loose").unwrap();
        let v = Vfs::open(&dir).unwrap();
        assert_eq!(v.pk3_count(), 2);
        assert_eq!(v.read("shaders/a.shader").unwrap().as_deref(), Some(&text[..]), "deflated, any case");
        assert_eq!(v.read("TEXTURES/a/b.jpg").unwrap().as_deref(), Some(&b"one"[..]), "the later package wins");
        assert_eq!(v.read("scripts/x.txt").unwrap().as_deref(), Some(&b"loose"[..]), "a loose file wins");
        assert!(v.read("textures/none.tga").unwrap().is_none());
        assert!(v.exists("textures/a/b.jpg") && !v.exists("textures/a/c.jpg"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
