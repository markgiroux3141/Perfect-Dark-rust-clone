//! Where a build's textures come from. The level's materials name ROM textures by preset number
//! (`complex::PALETTE`), and the OBJ and GLB writers read them as the GoldenEye Setup Editor
//! dumps them: `tempImgEd<PRESET>.bmp`, rows bottom to top, 32 bits a pixel, the fourth byte the
//! alpha where the texture has one.
//!
//! The game keeps the same textures in its pool (`assets/textures/<preset>.png`, the preset in
//! lower case, the image upside down from the dump: the N64's row order). `dumps` makes the
//! dumps a level needs from a pool, beside the level, so the writers read one kind of file.
//! Nothing ROM-derived is in this crate: both folders are given to it.

use std::fs;
use std::path::{Path, PathBuf};

use crate::obj::texture_file;

/// The pool's file for a preset.
pub fn pool_file(preset: &str) -> String {
    format!("{}.png", preset.to_ascii_lowercase())
}

/// Is `dir` the game's texture pool (and not a folder of dumps)? It is if it holds a pool file
/// for one of `presets` and no dump for any.
pub fn is_pool(dir: &Path, presets: &[&str]) -> bool {
    !presets.iter().any(|p| dir.join(texture_file(p)).is_file()) && presets.iter().any(|p| dir.join(pool_file(p)).is_file())
}

/// A pool image as the Setup Editor would dump it: a 32-bit BMP, rows bottom to top (which
/// turns the pool's image the right way up), the fourth byte the alpha, or 0 throughout when
/// the texture is opaque (as the editor leaves it).
pub fn dump(png: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).map_err(|e| e.to_string())?.into_rgba8();
    let (w, h) = (img.width(), img.height());
    let clear = img.pixels().any(|p| p.0[3] != 255);
    let size = w as usize * h as usize * 4;
    let mut out = Vec::with_capacity(54 + size);
    out.extend(b"BM");
    out.extend((54 + size as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(54u32.to_le_bytes());
    // BITMAPINFOHEADER: 40 bytes, one plane, 32 bits, no compression, 72 dots an inch
    out.extend(40u32.to_le_bytes());
    out.extend((w as i32).to_le_bytes());
    out.extend((h as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((size as u32).to_le_bytes());
    out.extend(2834i32.to_le_bytes());
    out.extend(2834i32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    // A BMP's first row is the picture's bottom one, and the pool's first row is too: the pool
    // keeps the image upside down, so its rows go out in the order they are in.
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        out.extend([b, g, r, if clear { a } else { 0 }]);
    }
    Ok(out)
}

/// The folder a build reads its textures from. A folder of dumps is used as it is. From the
/// game's pool, the dumps for `presets` are written into `out` (the level's own folder, where
/// the OBJ wants them beside it) and `out` is the answer; a preset the pool lacks is left for
/// the writers to warn about.
pub fn dumps(dir: &Path, out: &Path, presets: &[&str]) -> Result<PathBuf, String> {
    if !is_pool(dir, presets) {
        return Ok(dir.to_path_buf());
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for preset in presets {
        let from = dir.join(pool_file(preset));
        if !from.is_file() {
            continue;
        }
        let png = fs::read(&from).map_err(|e| format!("{}: {e}", from.display()))?;
        let bmp = dump(&png).map_err(|e| format!("{}: {e}", from.display()))?;
        let to = out.join(texture_file(preset));
        fs::write(&to, bmp).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(out.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glb;

    fn png(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            p.0 = pixel(x, y);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_pool_image_is_dumped_the_other_way_up_with_its_alpha() {
        // the pool's row 0 is the picture's bottom row
        let glass = dump(&png(4, 2, |x, y| [x as u8 * 10, y as u8 * 100, 7, if y == 0 { 0 } else { 255 }])).unwrap();
        assert_eq!((glass.len(), &glass[..2], glass[28]), (54 + 32, &b"BM"[..], 32));
        // read back as the GLB writer reads a dump: rows top to bottom
        let p = glb::decode(&glass).unwrap();
        assert_eq!((p.width, p.height, p.alpha), (4, 2, true));
        assert_eq!(&p.data[..4], &[0, 100, 7, 255], "the top row is the pool's second");
        assert_eq!(&p.data[16 + 4..16 + 8], &[10, 0, 7, 0], "the bottom row is the pool's first, clear");
        // an opaque texture's fourth byte is 0, which the reader takes as never used
        let tile = dump(&png(2, 2, |x, y| [x as u8, y as u8, 3, 255])).unwrap();
        assert!(tile[54..].chunks(4).all(|c| c[3] == 0));
        let p = glb::decode(&tile).unwrap();
        assert!(p.data.chunks(4).all(|c| c[3] == 255), "so it is read back opaque");
        assert!(dump(b"not a png").is_err());
    }

    #[test]
    fn dumps_are_made_from_a_pool_and_a_folder_of_dumps_is_used_as_it_is() {
        let dir = std::env::temp_dir().join(format!("autolevel_textures_{}", std::process::id()));
        let (pool, out, dumped) = (dir.join("pool"), dir.join("out"), dir.join("dumped"));
        for d in [&pool, &dumped] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(pool.join("073e.png"), png(2, 2, |_, _| [1, 2, 3, 255])).unwrap();
        fs::write(pool.join("0190.png"), png(2, 2, |_, _| [1, 2, 3, 128])).unwrap();
        let presets = ["073E", "0190", "0748"];
        assert!(is_pool(&pool, &presets) && !is_pool(&out, &presets));
        assert_eq!(dumps(&pool, &out, &presets).unwrap(), out);
        assert!(out.join("tempImgEd073E.bmp").is_file() && out.join("tempImgEd0190.bmp").is_file());
        assert!(!out.join("tempImgEd0748.bmp").exists(), "the pool has no 0748: left for the writers to warn about");
        // what was just written is a folder of dumps
        assert!(!is_pool(&out, &presets));
        assert_eq!(dumps(&out, &dumped, &presets).unwrap(), out);
        assert_eq!(fs::read_dir(&dumped).unwrap().count(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
