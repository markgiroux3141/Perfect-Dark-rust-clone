//! The variants strip (the editing roadmap's item 5): re-rolls of one room's whole inside or of
//! one part of it (catwalks, ramps, cover, windows), made side by side on threads, a small
//! picture each, and one kept with a click. The re-rolls are `autolevel::variants`; this is the
//! editor's half: the tiles and their levels (`Strip`, made on a thread as a `Task`), the room's
//! part of the plan cut out for a small picture (`prims`, `bounds`), and the strip as a PNG for
//! `--variants ... --shot` (`render`). The window draws the same tiles with egui (`ui`).
//!
//! A strip is of a recipe with the room's re-roll of that kind taken out (`Strip::base`): each
//! tile is that recipe with one re-roll put in, the one it has now being the first tile. So
//! keeping a tile leaves the others good (the recipe's base is the same), and any other change
//! to the recipe makes the strip stale (`fits`).

use crate::deadlist::{make, Made};
use crate::jobs::Task;
use crate::planview::{colour, picture, Prim, Rgba, View, P};
use crate::raster::{rasterise, Fit, Image, EDGE};
use autolevel::choices::{self, PART_KINDS};
use autolevel::level::{Level, PARTS};
use autolevel::recipe::Recipe;
use autolevel::variants::{self, side_by_side, Variant, SHOWN};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

/// One re-roll on the strip: what it is, and its level or why it makes none.
#[derive(Clone)]
pub struct Tile {
    pub variant: Variant,
    pub made: Result<Arc<Made>, String>,
    /// The room's part came out as it is now (no change to see).
    pub same: bool,
    /// Its small picture, worked out once: the shapes and the bounds they are cut to.
    pub pic: Option<Pic>,
}

/// A small picture: the plan's shapes, and the part of it (cells) it shows.
pub type Pic = (Vec<Prim>, (P, P));

/// `room`'s `part` in `lv` as a small picture (None for a room `lv` lacks).
pub fn pic(lv: &Level, room: &str, part: Option<&str>) -> Option<Pic> {
    Some((prims(lv, storey(lv, room, part)), bounds(lv, room)?))
}

/// What the strip's thread makes: the re-rolls and their levels.
type Tiles = Vec<(Variant, Result<Made, String>)>;

/// Re-rolls of one room's part being shown.
pub struct Strip {
    pub room: String,
    /// One of `PARTS`, or None for the whole inside.
    pub part: Option<&'static str>,
    /// The first stream asked for (the one the room has now is never among them).
    pub from: u32,
    /// The recipe the tiles are re-rolls of: the room's re-roll of this kind taken out.
    pub base: Recipe,
    pub tiles: Vec<Tile>,
    /// Why there are none (the re-roll can't be asked for).
    pub error: Option<String>,
    /// The room as it was when the strip was opened, as a tile (its stream, the recipe, the
    /// level): what a click has back. None on the command line.
    pub was: Option<Tile>,
    task: Option<Task<Tiles>>,
}

/// The kind of choice a re-roll of `part` is (`choices::kind`): "reroll", or "reroll catwalks".
pub fn kind(part: Option<&str>) -> &'static str {
    match part.and_then(|p| PARTS.iter().position(|q| *q == p)) {
        Some(i) => PART_KINDS[i],
        None => "reroll",
    }
}

/// A part by its name, as `PARTS` has it (None for "inside" or anything else).
pub fn part(name: &str) -> Option<&'static str> {
    PARTS.iter().find(|p| **p == name).copied()
}

/// The re-rolls of `room`'s part from stream `from` and their levels, made side by side. `level`
/// (the recipe's, if made) refuses a pod or a room it lacks before anything is made.
pub fn tiles(recipe: &Recipe, level: Option<&Level>, room: &str, part: Option<&str>, from: u32) -> Result<Tiles, String> {
    let vs = variants::variants(recipe, level, room, part, from, SHOWN)?;
    let made = side_by_side(&vs, |v| catch_unwind(AssertUnwindSafe(|| make(&v.recipe))).unwrap_or_else(|_| Err("the generator stopped on it".to_string())));
    Ok(vs.into_iter().zip(made).collect())
}

impl Strip {
    /// Start making the re-rolls of `room`'s part in `recipe` (on a thread), from stream `from`;
    /// `made`, the recipe's level if it is made, refuses a pod or a room it lacks at once.
    pub fn start(recipe: &Recipe, made: Option<Arc<Made>>, room: &str, part: Option<&'static str>, from: u32) -> Strip {
        let (r, key, had) = (recipe.clone(), room.to_string(), made.clone());
        let task = Task::run(0, move || tiles(&r, had.as_ref().map(|m| &m.level), &key, part, from));
        let n = variants::now(recipe, room, part);
        let was = made.map(|m| Tile {
            variant: Variant { n, edit: variants::edit(room, part, n), recipe: recipe.clone() },
            pic: pic(&m.level, room, part),
            made: Ok(m),
            same: true,
        });
        Strip { task: Some(task), was, ..Strip::start_idle(recipe, room, part, from) }
    }

    /// The same strip's next re-rolls (the streams after its last), on a thread.
    pub fn more(&self, recipe: &Recipe) -> Strip {
        let from = self.tiles.last().map_or(self.from, |t| t.variant.n + 1);
        Strip { was: self.was.clone(), ..Strip::start(recipe, None, &self.room, self.part, from) }
    }

    /// The re-rolls made here and now (the command line's, and the tests').
    pub fn now(recipe: &Recipe, room: &str, part: Option<&'static str>, from: u32, level: Option<&Level>) -> Strip {
        let mut s = Strip::start_idle(recipe, room, part, from);
        s.take(tiles(recipe, level, room, part, from), level);
        s
    }

    fn start_idle(recipe: &Recipe, room: &str, part: Option<&'static str>, from: u32) -> Strip {
        Strip { room: room.to_string(), part, from, base: choices::clear(recipe, room, kind(part)), tiles: Vec::new(), error: None, was: None, task: None }
    }

    /// Still being made?
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }

    /// Take the tiles once they are made; `level` is the recipe's level as it is now, to tell a
    /// re-roll that came out the same. True when they came in.
    pub fn poll(&mut self, level: Option<&Level>) -> bool {
        let Some(result) = self.task.as_ref().and_then(Task::poll) else { return false };
        self.task = None;
        self.take(result, level);
        true
    }

    fn take(&mut self, result: Result<Tiles, String>, level: Option<&Level>) {
        match result {
            Ok(made) => {
                let had = level.map(|lv| variants::part_of(lv, &self.room, self.part));
                self.tiles = made
                    .into_iter()
                    .map(|(variant, made)| {
                        let same = matches!((&made, &had), (Ok(m), Some(had)) if variants::part_of(&m.level, &self.room, self.part) == *had);
                        let pic = made.as_ref().ok().and_then(|m| pic(&m.level, &self.room, self.part));
                        Tile { variant, made: made.map(Arc::new), same, pic }
                    })
                    .collect();
                self.error = None;
            }
            Err(why) => self.error = Some(why),
        }
    }

    /// Are the tiles still re-rolls of `recipe` (it differs from what they were made from in
    /// this room's re-roll of this kind at most)?
    pub fn fits(&self, recipe: &Recipe) -> bool {
        choices::clear(recipe, &self.room, kind(self.part)) == self.base
    }

    /// The stream the room's part is on in `recipe`.
    pub fn had(&self, recipe: &Recipe) -> u32 {
        variants::now(recipe, &self.room, self.part)
    }

    /// The strip's heading: "vast's catwalks: now the seed's own".
    pub fn heading(&self, recipe: &Recipe) -> String {
        let what = self.part.unwrap_or("inside");
        format!("{}'s {what}: now {}", self.room, variants::label(self.part, self.had(recipe)))
    }

    /// A tile's caption: "catwalks 3", "catwalks 4 (the same)", "catwalks 5 (no level)".
    pub fn caption(&self, t: &Tile) -> String {
        let name = variants::label(self.part, t.variant.n);
        match Strip::note(t) {
            "" => name,
            note => format!("{name} ({note})"),
        }
    }

    /// What a tile's picture can't say: "the same" (as the room is now), "no level", or "".
    pub fn note(t: &Tile) -> &'static str {
        match &t.made {
            Err(_) => "no level",
            Ok(_) if t.same => "the same",
            Ok(_) => "",
        }
    }
}

/// The part of the plan a small picture of `room` shows: its box and two cells round it (a
/// window's corridor, a door's stub), in cells.
pub fn bounds(lv: &Level, room: &str) -> Option<(P, P)> {
    let s = lv.spaces().iter().find(|s| s.key == room)?;
    Some(((s.x0 as f32 - 2.0, s.y0 as f32 - 2.0), (s.x1 as f32 + 2.0, s.y1 as f32 + 2.0)))
}

/// The storey a small picture of `room`'s part shows: its floor alone for its cover (decks over
/// it would hide it), else every storey (None).
pub fn storey(lv: &Level, room: &str, part: Option<&str>) -> Option<i32> {
    (part == Some("cover")).then(|| lv.spaces().iter().find(|s| s.key == room).map(|s| s.floor)).flatten()
}

/// The plan of `lv` for a small picture (one storey, or every storey with the lowest
/// underneath): its shapes alone, no words (they would not fit).
pub fn prims(lv: &Level, storey: Option<i32>) -> Vec<Prim> {
    let view = View { storey, scale: 6.0, ..View::default() };
    picture(lv, &view).prims.into_iter().filter(|p| !matches!(p, Prim::Text { .. } | Prim::Badge { .. })).collect()
}

/// A tile's picture, `w` by `h` pixels: `room`'s `part` in `lv`.
pub fn thumb(lv: &Level, room: &str, part: Option<&str>, w: usize, h: usize) -> Image {
    match bounds(lv, room) {
        Some((lo, hi)) => rasterise(&prims(lv, storey(lv, room, part)), &Fit::inside(lo, hi, w as f32, h as f32), w, h, colour::BACKGROUND),
        None => Image::new(w, h, colour::BACKGROUND),
    }
}

/// The colour a kept or lit tile is framed in.
pub const LIT: Rgba = [255, 196, 64, 255];

/// The strip as a PNG: the room as it is now (`now`), then each tile, side by side, each with
/// its number (what `--keep N` takes) and caption, under a heading.
pub fn render(strip: &Strip, now: &Made, recipe: &Recipe) -> Image {
    const W: usize = 260;
    const H: usize = 200;
    const GAP: usize = 10;
    let n = strip.tiles.len() + 1;
    let (width, height) = (GAP + n * (W + GAP), 30 + H + 40 + GAP);
    let mut img = Image::new(width, height, [10, 11, 15, 255]);
    let white: Rgba = [230, 232, 238, 255];
    let words = match &strip.error {
        Some(why) => format!("{}: {why}", strip.heading(recipe)),
        None => format!("{}   (--keep N keeps number N)", strip.heading(recipe)),
    };
    img.text(GAP as i64, 9, &words, 2, white, Some(EDGE));
    // (a caption, and under it what the picture can't say)
    let shown =
        std::iter::once((format!("now: {}", variants::label(strip.part, strip.had(recipe))), "", Some(&now.level), true)).chain(
            strip.tiles.iter().enumerate().map(|(i, t)| {
                (format!("{}  {}", i + 1, variants::label(strip.part, t.variant.n)), Strip::note(t), t.made.as_ref().ok().map(|m| &m.level), false)
            }),
        );
    for (k, (caption, note, lv, lit)) in shown.enumerate() {
        let x = GAP + k * (W + GAP);
        let pic = match lv {
            Some(lv) => thumb(lv, &strip.room, strip.part, W, H),
            None => {
                let mut pic = Image::new(W, H, [40, 16, 18, 255]);
                pic.text(10, H as i64 / 2 - 8, "no level", 2, [255, 120, 120, 255], Some(EDGE));
                pic
            }
        };
        if lit {
            img.fill(x as i64 - 3, 27, W as i64 + 6, H as i64 + 6, LIT);
        }
        img.paste(&pic, x, 30);
        img.text(x as i64, (30 + H + 6) as i64, &caption, 2, white, Some(EDGE));
        img.text(x as i64 + 24, (30 + H + 24) as i64, note, 2, [150, 154, 166, 255], Some(EDGE));
    }
    img
}

/// The strip in words, a line a tile, numbered as `--keep` takes them.
pub fn text(strip: &Strip, recipe: &Recipe) -> String {
    let mut out = format!("variants: {}\n", strip.heading(recipe));
    if let Some(why) = &strip.error {
        out.push_str(&format!("  refused: {why}\n"));
    }
    for (i, t) in strip.tiles.iter().enumerate() {
        let why = t.made.as_ref().err().map_or(String::new(), |why| format!(" ({why})"));
        out.push_str(&format!("  {}  {}{why}\n", i + 1, strip.caption(t)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    #[test]
    fn a_strip_is_made_of_the_next_streams_and_stays_good_when_one_is_kept() {
        let made = fixture::seed40();
        let r = Recipe::python(40);
        let s = Strip::now(&r, "colossal", Some("catwalks"), 1, Some(&made.level));
        assert_eq!(s.tiles.iter().map(|t| t.variant.n).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
        assert!(s.tiles.iter().all(|t| t.made.is_ok()));
        assert!(s.tiles.iter().any(|t| !t.same), "some re-roll changes colossal's catwalks");
        assert_eq!(s.heading(&r), "colossal's catwalks: now the seed's own");
        // a tile kept: the strip still fits, and says what it has now
        let kept = choices::set(&r, s.tiles[2].variant.edit.clone()).unwrap();
        assert_eq!(kept, s.tiles[2].variant.recipe);
        assert!(s.fits(&kept) && s.fits(&r));
        assert_eq!(s.heading(&kept), "colossal's catwalks: now catwalks 3");
        // any other change: stale
        let other = choices::set(&r, autolevel::recipe::Edit::Cover { room: "vast".into(), cover: "none".into() }).unwrap();
        assert!(!s.fits(&other));
        // the picture: the room now, then a tile each
        let img = render(&s, made, &r);
        assert_eq!((img.w, img.h), (10 + 6 * 270, 30 + 200 + 40 + 10));
        assert!(text(&s, &r).starts_with("variants: colossal's catwalks: now the seed's own\n  1  catwalks 1"));
        // refused before anything is made
        let pod = Strip::now(&r, "small8", None, 1, Some(&made.level));
        assert_eq!(pod.error.as_deref(), Some("reroll: small8 is a pod of vast: it is not furnished by itself"));
        assert_eq!((kind(None), kind(Some("ramps")), part("inside"), part("cover")), ("reroll", "reroll ramps", None, Some("cover")));
    }
}
