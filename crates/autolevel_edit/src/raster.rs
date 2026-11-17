//! Lists of primitives to pixels: an RGBA image, the shapes drawn into it, and the PNG. This is
//! the snapshot's half of the plan view (the window draws the same lists with egui), after
//! `pd_tools::lab::rasterise` in the clone, which leans on that workspace's software rasteriser;
//! here the few shapes needed are written out, and text is drawn (`font`).
//!
//! Shapes are drawn at twice the size and halved, which smooths their edges; text goes on last,
//! at the final size, each letter with a dark edge so it reads over any colour.

use crate::font;
use crate::planview::{Prim, Rgba, P};

/// An RGBA8 image, rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Image {
    /// An image of one colour.
    pub fn new(w: usize, h: usize, bg: Rgba) -> Image {
        Image { w, h, px: bg.iter().copied().cycle().take(w * h * 4).collect() }
    }

    pub fn get(&self, x: usize, y: usize) -> Rgba {
        let i = (y * self.w + x) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    /// Lay a colour over a pixel by its alpha (outside the image: nothing).
    pub fn blend(&mut self, x: i64, y: i64, c: Rgba) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 4;
        let a = c[3] as u32;
        for (px, v) in self.px[i..i + 3].iter_mut().zip(c) {
            *px = ((v as u32 * a + *px as u32 * (255 - a) + 127) / 255) as u8;
        }
        self.px[i + 3] = 255;
    }

    /// Pixels x0 to x1 (not x1 itself) of row y.
    fn span(&mut self, y: i64, x0: i64, x1: i64, c: Rgba) {
        if y < 0 || y >= self.h as i64 {
            return;
        }
        for x in x0.max(0)..x1.min(self.w as i64) {
            self.blend(x, y, c);
        }
    }

    /// A rectangle of pixels.
    pub fn fill(&mut self, x: i64, y: i64, w: i64, h: i64, c: Rgba) {
        for row in y..y + h {
            self.span(row, x, x + w, c);
        }
    }

    /// A filled polygon (any simple one): the pixels whose centres are inside it.
    pub fn poly(&mut self, pts: &[P], c: Rgba) {
        if pts.len() < 3 {
            return;
        }
        let top = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let bottom = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        let (y0, y1) = ((top - 0.5).ceil().max(0.0) as i64, ((bottom - 0.5).ceil() as i64).min(self.h as i64));
        let mut xs: Vec<f32> = Vec::new();
        for y in y0..y1 {
            let sy = y as f32 + 0.5;
            xs.clear();
            for (k, a) in pts.iter().enumerate() {
                let b = pts[(k + 1) % pts.len()];
                if (a.1 <= sy) != (b.1 <= sy) {
                    xs.push(a.0 + (sy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                }
            }
            xs.sort_by(f32::total_cmp);
            for pair in xs.chunks_exact(2) {
                self.span(y, (pair[0] - 0.5).ceil() as i64, (pair[1] - 0.5).ceil() as i64, c);
            }
        }
    }

    /// A line `width` pixels thick.
    pub fn line(&mut self, a: P, b: P, width: f32, c: Rgba) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            return;
        }
        let (nx, ny) = (-dy / len * width * 0.5, dx / len * width * 0.5);
        self.poly(&[(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)], c);
    }

    /// A disc, or a ring `stroke` pixels thick.
    pub fn circle(&mut self, at: P, r: f32, c: Rgba, filled: bool, stroke: f32) {
        let inner = if filled { -1.0 } else { (r - stroke).max(0.0) };
        for y in (at.1 - r).floor() as i64..=(at.1 + r).ceil() as i64 {
            for x in (at.0 - r).floor() as i64..=(at.0 + r).ceil() as i64 {
                let (dx, dy) = (x as f32 + 0.5 - at.0, y as f32 + 0.5 - at.1);
                let d = (dx * dx + dy * dy).sqrt();
                if d <= r && d > inner {
                    self.blend(x, y, c);
                }
            }
        }
    }

    /// A line of text with its top left corner at (x, y), each dot `k` pixels square, edged
    /// with `edge` (None: no edge).
    pub fn text(&mut self, x: i64, y: i64, text: &str, k: usize, c: Rgba, edge: Option<Rgba>) {
        let k = k.max(1) as i64;
        let dots = |f: &mut dyn FnMut(i64, i64)| {
            for (n, ch) in text.chars().enumerate() {
                for col in 0..font::W {
                    for row in 0..font::ROWS {
                        if font::dot(ch, col, row) {
                            f(x + (n * font::ADVANCE + col) as i64 * k, y + row as i64 * k);
                        }
                    }
                }
            }
        };
        if let Some(edge) = edge {
            // (the edge first, whole, so no letter's edge lies over its neighbour)
            let mut seen = std::collections::BTreeSet::new();
            dots(&mut |px, py| {
                for ey in py - 1..py + k + 1 {
                    for ex in px - 1..px + k + 1 {
                        seen.insert((ex, ey));
                    }
                }
            });
            for (ex, ey) in seen {
                self.blend(ex, ey, edge);
            }
        }
        dots(&mut |px, py| self.fill(px, py, k, k, c));
    }

    /// The image at half the size: each pixel the mean of four.
    pub fn half(&self) -> Image {
        let (w, h) = (self.w / 2, self.h / 2);
        let mut px = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                for k in 0..4 {
                    let at = |dx: usize, dy: usize| self.px[((2 * y + dy) * self.w + 2 * x + dx) * 4 + k] as u32;
                    px.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
                }
            }
        }
        Image { w, h, px }
    }

    /// Another image laid over this one with its top left corner at (x, y).
    pub fn paste(&mut self, other: &Image, x: usize, y: usize) {
        for row in 0..other.h.min(self.h.saturating_sub(y)) {
            let n = other.w.min(self.w.saturating_sub(x)) * 4;
            let (from, to) = (row * other.w * 4, ((y + row) * self.w + x) * 4);
            self.px[to..to + n].copy_from_slice(&other.px[from..from + n]);
        }
    }

    /// The image as a PNG file's bytes: the same image is always the same bytes.
    pub fn png(&self) -> Result<Vec<u8>, String> {
        use image::ImageEncoder;
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(&self.px, self.w as u32, self.h as u32, image::ExtendedColorType::Rgba8)
            .map_err(|e| format!("png: {e}"))?;
        Ok(out)
    }
}

/// How a picture in cells lies on pixels: north is up, so y turns over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The picture's west edge and north edge, cells.
    pub west: f32,
    pub north: f32,
    /// Pixels to a cell.
    pub scale: f32,
    /// The pixel its north-west corner falls on.
    pub x: f32,
    pub y: f32,
}

impl Fit {
    /// The bounds `lo` to `hi` at `scale` pixels a cell, the north-west corner at (x, y).
    pub fn new(lo: P, hi: P, scale: f32, x: f32, y: f32) -> Fit {
        Fit { west: lo.0, north: hi.1, scale, x, y }
    }

    /// The bounds as large as they go in a `w` by `h` image, in the middle of it.
    pub fn inside(lo: P, hi: P, w: f32, h: f32) -> Fit {
        let scale = (w / (hi.0 - lo.0)).min(h / (hi.1 - lo.1));
        Fit::new(lo, hi, scale, (w - (hi.0 - lo.0) * scale) / 2.0, (h - (hi.1 - lo.1) * scale) / 2.0)
    }

    /// A place in cells as a pixel.
    pub fn to(&self, p: P) -> P {
        (self.x + (p.0 - self.west) * self.scale, self.y + (self.north - p.1) * self.scale)
    }

    /// A pixel as a place in cells.
    pub fn from(&self, p: P) -> P {
        (self.west + (p.0 - self.x) / self.scale, self.north - (p.1 - self.y) / self.scale)
    }
}

/// The size of dot a text of `size` pixels is drawn with (the face is 7 dots high, 8 with air).
pub fn dot_size(size: f32) -> usize {
    ((size / 8.0).round() as usize).max(1)
}

/// The colour letters are edged with.
pub const EDGE: Rgba = [12, 13, 17, 210];

/// `prims` drawn on an image `w` by `h` of colour `bg`, as `fit` lays them.
pub fn rasterise(prims: &[Prim], fit: &Fit, w: usize, h: usize, bg: Rgba) -> Image {
    let big = Fit { scale: fit.scale * 2.0, x: fit.x * 2.0, y: fit.y * 2.0, ..*fit };
    let mut img = Image::new(w * 2, h * 2, bg);
    for p in prims {
        match p {
            Prim::Poly { pts, fill } => {
                let pts: Vec<P> = pts.iter().map(|&q| big.to(q)).collect();
                img.poly(&pts, *fill);
            }
            Prim::Line { a, b, col, width_px } => img.line(big.to(*a), big.to(*b), width_px * 2.0, *col),
            Prim::Circle { c, r, col, filled } => img.circle(big.to(*c), (r * big.scale).max(4.0), *col, *filled, 3.0),
            Prim::Text { .. } | Prim::Badge { .. } => {}
        }
    }
    let mut img = img.half();
    for p in prims {
        match p {
            Prim::Text { at, text, col, size_px, centred } => {
                let k = dot_size(*size_px);
                let (x, y) = fit.to(*at);
                let (tw, th) = ((font::width(text) * k) as f32, (font::H * k) as f32);
                let x = if *centred { x - tw / 2.0 } else { x };
                img.text(x.round() as i64, (y - th / 2.0).round() as i64, text, k, *col, Some(EDGE));
            }
            Prim::Badge { at, text, fill, col, r_px, size_px } => {
                // (a disc with a dark rim, its number plain: an edge would fill the disc)
                let (k, c) = (dot_size(*size_px), fit.to(*at));
                img.circle(c, *r_px, *col, true, 0.0);
                img.circle(c, r_px - 1.5, *fill, true, 0.0);
                let (tw, th) = ((font::width(text) * k) as f32, (font::H * k) as f32);
                img.text((c.0 - tw / 2.0).round() as i64, (c.1 - th / 2.0).round() as i64, text, k, *col, None);
            }
            _ => {}
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    const BG: Rgba = [0, 0, 0, 255];
    const RED: Rgba = [255, 0, 0, 255];
    const WHITE: Rgba = [255, 255, 255, 255];

    #[test]
    fn a_hand_made_list_has_the_pixels_expected() {
        // 10 cells square on 100 pixels: 10 pixels a cell, north up
        let fit = Fit::inside((0.0, 0.0), (10.0, 10.0), 100.0, 100.0);
        assert_eq!((fit.scale, fit.to((0.0, 10.0)), fit.to((10.0, 0.0)), fit.from((50.0, 20.0))), (10.0, (0.0, 0.0), (100.0, 100.0), (5.0, 8.0)));
        let prims = vec![
            // a red square in the north-west: cells 1..3 by 7..9
            Prim::Poly { pts: vec![(1.0, 7.0), (3.0, 7.0), (3.0, 9.0), (1.0, 9.0)], fill: RED },
            // half-clear white over its east half and beyond
            Prim::Poly { pts: vec![(2.0, 7.0), (4.0, 7.0), (4.0, 9.0), (2.0, 9.0)], fill: [255, 255, 255, 128] },
            // a line 4 pixels thick along y = 5 from x = 1 to x = 9
            Prim::Line { a: (1.0, 5.0), b: (9.0, 5.0), col: WHITE, width_px: 4.0 },
            // a disc of radius 1 cell at (7, 2), and a ring round (2, 2)
            Prim::Circle { c: (7.0, 2.0), r: 1.0, col: RED, filled: true },
            Prim::Circle { c: (2.0, 2.0), r: 1.0, col: WHITE, filled: false },
        ];
        let img = rasterise(&prims, &fit, 100, 100, BG);
        assert_eq!((img.w, img.h, img.px.len()), (100, 100, 40000));
        assert_eq!(img.get(15, 15), RED); // inside the square (y turned over: north is the top)
        assert_eq!(img.get(25, 20), [255, 128, 128, 255]); // red under half-clear white
        assert_eq!(img.get(35, 20), [128, 128, 128, 255]); // black under it
        assert_eq!(img.get(15, 35), BG); // south of the square
        assert_eq!(img.get(5, 15), BG); // west of it
        assert_eq!((img.get(50, 49), img.get(50, 51), img.get(50, 47), img.get(50, 53)), (WHITE, WHITE, BG, BG)); // the line, 4 thick
        assert_eq!((img.get(5, 50), img.get(95, 50)), (BG, BG)); // and no longer than it is
        assert_eq!((img.get(70, 80), img.get(77, 80), img.get(82, 80)), (RED, RED, BG)); // the disc
        assert_eq!((img.get(20, 80), img.get(29, 80), img.get(20, 70)), (BG, WHITE, WHITE)); // the ring: its middle clear
        let red = img.px.chunks(4).filter(|p| *p == RED).count();
        assert!((480..=520).contains(&red), "{red}"); // the square's clear half, 200, and the disc's area, about pi x 100
    }

    #[test]
    fn text_is_drawn_dot_for_dot() {
        let fit = Fit::new((0.0, 0.0), (10.0, 10.0), 10.0, 0.0, 0.0);
        let prims = vec![Prim::Text { at: (0.0, 9.0), text: "T".into(), col: WHITE, size_px: 8.0, centred: false }];
        let img = rasterise(&prims, &fit, 100, 100, BG);
        // the T's top bar is five dots wide at (0, 10 - 3.5 rounded); its stem runs down the middle
        let lit: Vec<(usize, usize)> = (0..100).flat_map(|y| (0..100).map(move |x| (x, y))).filter(|&(x, y)| img.get(x, y) == WHITE).collect();
        assert_eq!(lit.len(), 11);
        let (x0, y0) = (lit.iter().map(|p| p.0).min().unwrap(), lit.iter().map(|p| p.1).min().unwrap());
        assert_eq!((x0, y0), (0, 7));
        assert!((0..5).all(|x| lit.contains(&(x, 7))) && (7..14).all(|y| lit.contains(&(2, y))));
        // twice the size is every dot four pixels, and centred text sits about its point
        let prims = vec![Prim::Text { at: (5.0, 5.0), text: "T".into(), col: WHITE, size_px: 16.0, centred: true }];
        let img = rasterise(&prims, &fit, 100, 100, BG);
        let lit: Vec<(usize, usize)> = (0..100).flat_map(|y| (0..100).map(move |x| (x, y))).filter(|&(x, y)| img.get(x, y) == WHITE).collect();
        assert_eq!(lit.len(), 44);
        assert_eq!(
            (lit.iter().map(|p| p.0).min(), lit.iter().map(|p| p.0).max(), lit.iter().map(|p| p.1).min(), lit.iter().map(|p| p.1).max()),
            (Some(45), Some(54), Some(43), Some(56))
        );
        assert_eq!(dot_size(8.0), 1);
        assert_eq!((dot_size(11.0), dot_size(12.0), dot_size(16.0), dot_size(3.0)), (1, 2, 2, 1));
    }

    #[test]
    fn a_png_is_the_same_bytes_every_time_and_reads_back() {
        let mut img = Image::new(40, 30, [20, 22, 28, 255]);
        img.fill(5, 5, 10, 10, RED);
        img.text(2, 20, "seed 40", 1, WHITE, Some(EDGE));
        let (a, b) = (img.png().unwrap(), img.png().unwrap());
        assert_eq!(a, b);
        assert_eq!(&a[..8], b"\x89PNG\r\n\x1a\n");
        let back = image::load_from_memory(&a).unwrap().to_rgba8();
        assert_eq!((back.width(), back.height(), back.into_raw()), (40, 30, img.px.clone()));
        // halving averages, pasting copies
        let half = img.half();
        assert_eq!((half.w, half.h, half.get(4, 4)), (20, 15, RED));
        let mut wide = Image::new(60, 30, BG);
        wide.paste(&img, 20, 0);
        assert_eq!((wide.get(19, 6), wide.get(26, 6), wide.get(59, 29)), (BG, RED, [20, 22, 28, 255]));
    }
}
