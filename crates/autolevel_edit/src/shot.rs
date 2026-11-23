//! The plan view as a PNG, with no window and no GPU: `autolevel_edit --shot`. The same list of
//! primitives the window draws, rasterised, under a title (the level's numbers, as the Python's
//! plan.svg has them) and over the legend. This is how the plan is checked without anyone
//! looking at the window. A recipe that makes no level still has a picture: its room boxes on
//! the grid (`render_bare`), under the reason.

use crate::deadlist::Made;
use crate::font;
use crate::planview::{bare, bounds_bare, bounds_of, colour, picture, Key, Picture, Rgba, Swatch, View};
use crate::raster::{rasterise, Fit, Image, EDGE};
use crate::sideview::{self, SideView};
use autolevel::storeys::snap_top;

/// The title strip's height, and a legend row's, in pixels.
const TITLE: usize = 44;
const ROW: usize = 18;
/// The height of the line under the title that says what is picked.
const DOING: usize = 16;

/// The level's numbers in a line, as the Python titles its plan.
pub fn title(made: &Made) -> String {
    let r = &made.level.report;
    format!("autolevel seed {}: {} rooms, {} links, {} loops, score {}", r.seed, r.rooms, r.links, r.loops, r.score)
}

/// The dead ends in a line.
pub fn summary(made: &Made) -> String {
    let d = &made.dead;
    let long = d.left.iter().filter(|e| !e.end.allowed).count();
    let done = d.fixes.as_ref().map_or("left as they fell".to_string(), |f| format!("{} put right", f.len()));
    format!(
        "dead ends: {} as it fell, {} too long; {done}; left: {long} too long, {} allowed ({} camps)",
        d.before.len(),
        d.long(),
        d.left.len() - long,
        d.camps
    )
}

/// What the view has picked, in a line ("" if nothing): the room being joined and how many
/// corridors are drawn for it, the corridor picked, the one just taken out; and the room boxes,
/// if they are drawn: how many, which is selected, and that a change to them is refused (the
/// reason is on the plan, by the box).
pub fn doing(view: &View) -> String {
    let mut out: Vec<String> = Vec::new();
    if let Some((room, z)) = &view.pick {
        out.push(match view.joins.len() {
            0 => format!("picked: {room}, to join by a door on {z}: no corridor to draw"),
            1 => format!("picked: {room}, to join by a door on {z}: the corridor drawn pale could be made (numbered as it is listed)"),
            n => format!("picked: {room}, to join by a door on {z}: each of the {n} corridors drawn pale could be made (numbered as they are listed)"),
        });
    }
    if let Some(key) = &view.route {
        out.push(format!("picked: {key} (ringed in yellow)"));
    }
    if let Some(r) = &view.gone {
        out.push(format!("{} was taken out (in red, where it was)", r.key()));
    }
    match view.chosen.len() {
        0 => {}
        1 => out.push(format!("chosen by hand for {} (the words under its name)", view.chosen[0].0)),
        n => out.push(format!("chosen by hand for {n} rooms (the words under their names)")),
    }
    if let Some(b) = &view.boxes {
        let selected = b.selected.as_deref().and_then(|key| b.lots.iter().find(|a| a.key == key));
        let n = b.lots.len();
        out.push(match (&b.refused, selected) {
            (Some(_), Some(a)) => format!("room boxes: {n}; the change to {} is REFUSED (in red, the reason by its box)", a.key),
            (Some(_), None) => format!("room boxes: {n}; a change is REFUSED (the reason is in the corner)"),
            (None, Some(a)) => format!("room boxes: {n}; {} selected ({} x {} cells, {} to {})", a.key, a.x1 - a.x0, a.y1 - a.y0, a.floor, snap_top(a.top)),
            (None, None) => format!("room boxes: {n}, each edged in its storey's colour"),
        });
    }
    out.join("   ")
}

/// Which storeys the view shows, in words.
fn storeys(view: &View) -> String {
    match view.storey {
        None => "all storeys, the lowest underneath".to_string(),
        Some(z) => format!("storey {z} only"),
    }
}

/// What is under the title of every picture.
const NOTE: &str = "north is up; 1 cell = 118 units = 3 m; labels are room and route keys";

/// Where each legend row goes on a strip `width` wide: (x, row) for each key, and the rows used.
fn lay(legend: &[Key], width: usize) -> (Vec<(usize, usize)>, usize) {
    let (mut x, mut row, mut at) = (12, 0, Vec::new());
    for k in legend {
        let w = 22 + font::width(&k.text) + 20;
        if x + w > width && x > 12 {
            (x, row) = (12, row + 1);
        }
        at.push((x, row));
        x += w;
    }
    (at, row + 1)
}

fn swatch(img: &mut Image, x: i64, y: i64, k: &Key) {
    let edge: Rgba = [200, 204, 214, 255];
    match k.swatch {
        Swatch::Fill => {
            img.fill(x, y, 16, 10, edge);
            img.fill(x + 1, y + 1, 14, 8, k.col);
        }
        Swatch::Line => img.fill(x, y + 3, 16, 4, k.col),
        Swatch::Ring => img.circle((x as f32 + 8.0, y as f32 + 5.0), 6.0, k.col, false, 2.0),
    }
}

/// The plan of a level, `width` pixels wide, as `view` asks for it: all storeys or one, what is
/// picked, the room boxes. (The view's scale is the picture's own, whatever it says.)
pub fn render(made: &Made, view: &View, width: usize) -> Image {
    let width = width.clamp(320, 8000);
    let (lo, hi) = bounds_of(&made.level, view);
    let view = &View { scale: width as f32 / (hi.0 - lo.0), ..view.clone() };
    let picked = if view.mark.is_some() { "  (one row of the list picked out)" } else { "" };
    let head = format!("{}   [{}]{picked}", title(made), storeys(view));
    compose(&picture(&made.level, view), view, width, &head, &format!("{}   {NOTE}", summary(made)))
}

/// The room boxes of a recipe that makes no level (`why`: the generator's own words), on the
/// grid alone, `width` pixels wide: `view.boxes`, with what is selected or refused.
pub fn render_bare(seed: u64, why: &str, view: &View, width: usize) -> Image {
    let width = width.clamp(320, 8000);
    let (lo, hi) = bounds_bare(view);
    let view = &View { scale: width as f32 / (hi.0 - lo.0), ..view.clone() };
    let head = format!("autolevel seed {seed}: no level, the room boxes alone   [{}]", storeys(view));
    compose(&bare(view), view, width, &head, &format!("no level: {why}   {NOTE}"))
}

/// The side view (`sideview`) `width` pixels wide: of the level if there is one (`made`), else of
/// the room boxes alone, under `under` (the generator's reason, for no level).
pub fn render_side(made: Option<&Made>, seed: u64, under: &str, side: &SideView, width: usize) -> Image {
    let width = width.clamp(320, 8000);
    let lv = made.map(|m| &m.level);
    let (lo, hi) = sideview::bounds(lv, side);
    let side = &SideView { scale: width as f32 / (hi.0 - lo.0), ..side.clone() };
    let cut = side.section.map_or("no section".to_string(), |s| s.words());
    let head = match made {
        Some(made) => format!("{}   [side view {cut}]", title(made)),
        None => format!("autolevel seed {seed}: no level, the room boxes alone   [side view {cut}]"),
    };
    let view = View { boxes: side.boxes.clone(), ..View::default() };
    compose(&sideview::picture(lv, side), &view, width, &head, under)
}

/// A picture `width` pixels wide under its two title lines (and what the view is doing, if
/// anything) and over its legend.
fn compose(pic: &Picture, view: &View, width: usize, head: &str, under: &str) -> Image {
    let scale = width as f32 / (pic.hi.0 - pic.lo.0);
    let plan_h = ((pic.hi.1 - pic.lo.1) * scale).ceil() as usize;
    let fit = Fit::new(pic.lo, pic.hi, scale, 0.0, 0.0);
    let plan = rasterise(&pic.prims, &fit, width, plan_h, colour::BACKGROUND);
    let (at, rows) = lay(&pic.legend, width);
    let legend_h = rows * ROW + 10;
    // (what is picked gets a line of its own under the title)
    let doing = doing(view);
    let top = TITLE + if doing.is_empty() { 0 } else { DOING };
    let mut img = Image::new(width, top + plan_h + legend_h, colour::BACKGROUND);
    img.paste(&plan, 0, top);
    img.text(12, 7, head, 2, colour::TITLE, Some(EDGE));
    img.text(12, 29, under, 1, colour::FAINT, Some(EDGE));
    let refused = view.boxes.as_ref().is_some_and(|b| b.refused.is_some());
    img.text(12, 45, &doing, 1, if refused { colour::DEAD_END } else { colour::PICK }, Some(EDGE));
    for (k, (x, row)) in pic.legend.iter().zip(at) {
        let y = (top + plan_h + 6 + row * ROW) as i64;
        swatch(&mut img, x as i64, y, k);
        img.text(x as i64 + 22, y + 2, &k.text, 1, colour::KEY, Some(EDGE));
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    #[test]
    fn a_shot_is_the_same_bytes_every_time() {
        let made = fixture::seed40();
        let all = View::default();
        let first = render(made, &all, 800);
        let (a, b) = (first.png().unwrap(), render(made, &all, 800).png().unwrap());
        assert_eq!(a, b);
        assert_eq!(first.w, 800);
        // the plan, with its title over it and its legend under it
        let (lo, hi) = crate::planview::bounds(&made.level);
        let plan = ((hi.1 - lo.1) * 800.0 / (hi.0 - lo.0)).ceil() as usize;
        assert!(first.h > TITLE + plan && first.h <= TITLE + plan + 4 * ROW + 10);
        let lit = |y0: usize, y1: usize| (y0..y1).any(|y| (0..800).any(|x| first.get(x, y) == colour::TITLE || first.get(x, y) == colour::KEY));
        assert!(lit(0, TITLE) && lit(TITLE + plan, first.h));
        // another storey, or a picked row, is another picture
        assert_ne!(render(made, &View { storey: Some(276), ..View::default() }, 800).png().unwrap(), a);
        assert_ne!(render(made, &View { mark: Some(made.dead.left[0].mark.clone()), ..View::default() }, 800).png().unwrap(), a);
        assert_eq!(title(made), "autolevel seed 40: 29 rooms, 44 links, 16 loops, score 37.85");
        assert_eq!(summary(made), "dead ends: 11 as it fell, 2 too long; 2 put right; left: 0 too long, 10 allowed (6 camps)");
    }

    fn boxed(selected: Option<&str>, refused: Option<&str>) -> View {
        let lots = crate::doc::Doc::new(autolevel::recipe::Recipe::python(40), true).lots().unwrap().lots;
        View {
            boxes: Some(crate::planview::Boxes { lots, selected: selected.map(str::to_string), refused: refused.map(str::to_string), site: None }),
            ..View::default()
        }
    }

    #[test]
    fn the_room_boxes_are_in_the_shot_and_its_title_says_what_is_done_to_them() {
        let made = fixture::seed40();
        assert_eq!(doing(&View::default()), "");
        assert_eq!(doing(&boxed(None, None)), "room boxes: 29, each edged in its storey's colour");
        assert_eq!(doing(&boxed(Some("medium1"), None)), "room boxes: 29; medium1 selected (7 x 5 cells, 0 to 512)");
        assert_eq!(
            doing(&boxed(Some("medium1"), Some("medium1 and small3 overlap"))),
            "room boxes: 29; the change to medium1 is REFUSED (in red, the reason by its box)"
        );
        assert_eq!(doing(&boxed(None, Some("move: no room attic"))), "room boxes: 29; a change is REFUSED (the reason is in the corner)");
        // (with a corridor picked too, both are said)
        let both = View { route: Some("c23".into()), ..boxed(None, None) };
        assert_eq!(doing(&both), "picked: c23 (ringed in yellow)   room boxes: 29, each edged in its storey's colour");
        // each is another picture, a line taller than the plain plan for what is being done
        let png = |view: &View| render(made, view, 800).png().unwrap();
        let (plain, rooms, selected, refused) =
            (png(&View::default()), png(&boxed(None, None)), png(&boxed(Some("medium1"), None)), png(&boxed(Some("medium1"), Some("no"))));
        assert!(plain != rooms && rooms != selected && selected != refused);
        // (and a row or two of legend)
        let more = render(made, &boxed(None, None), 800).h - render(made, &View::default(), 800).h;
        assert!(more >= DOING && (more - DOING).is_multiple_of(ROW) && more <= DOING + 2 * ROW, "{more}");
        // the line is in the pick colour, or in red for a refusal
        let has = |view: &View, c: Rgba| {
            let img = render(made, view, 800);
            (TITLE..TITLE + DOING).any(|y| (0..800).any(|x| img.get(x, y) == c))
        };
        assert!(has(&boxed(Some("medium1"), None), colour::PICK) && !has(&boxed(Some("medium1"), None), colour::DEAD_END));
        assert!(has(&boxed(Some("medium1"), Some("no")), colour::DEAD_END));
    }

    #[test]
    fn the_title_says_for_how_many_rooms_something_was_chosen() {
        let made = fixture::seed40();
        let said = |room: &str, words: &str| (room.to_string(), words.to_string());
        let one = View { chosen: vec![said("vast", "cover none")], ..View::default() };
        let two = View { chosen: vec![said("vast", "cover none"), said("colossal", "layout ring")], ..View::default() };
        assert_eq!(doing(&one), "chosen by hand for vast (the words under its name)");
        assert_eq!(doing(&two), "chosen by hand for 2 rooms (the words under their names)");
        assert_eq!(
            doing(&View { chosen: two.chosen.clone(), ..boxed(None, None) }),
            format!("{}   room boxes: 29, each edged in its storey's colour", doing(&two))
        );
        // the words are in the picture: each is another one
        let png = |view: &View| render(made, view, 800).png().unwrap();
        assert!(png(&one) != png(&View::default()) && png(&one) != png(&two));
    }

    #[test]
    fn a_recipe_that_makes_no_level_still_has_a_picture_of_its_boxes() {
        let view = boxed(Some("tower"), None);
        let img = render_bare(40, "colossal: no set of ramps joins its storeys", &view, 800);
        // the boxes' own bounds at the width asked for, under the title and over a legend
        let (lo, hi) = bounds_bare(&view);
        let plan = ((hi.1 - lo.1) * 800.0 / (hi.0 - lo.0)).ceil() as usize;
        let legend = img.h - (TITLE + DOING + plan + 10);
        assert!(img.w == 800 && legend.is_multiple_of(ROW) && (ROW..=3 * ROW).contains(&legend), "{} x {}", img.w, img.h);
        assert_eq!(img.png().unwrap(), render_bare(40, "colossal: no set of ramps joins its storeys", &view, 800).png().unwrap());
        // the boxes are drawn in their storeys' colours, and the selected one's edge in the pick colour
        let has = |img: &Image, c: Rgba| (TITLE + DOING..TITLE + DOING + plan).any(|y| (0..800).any(|x| img.get(x, y) == c));
        assert!(has(&img, colour::PICK) && has(&img, colour::GRIP));
        let plain = render_bare(40, "no", &boxed(None, None), 800);
        assert!(!has(&plain, colour::PICK) && plain.png().unwrap() != img.png().unwrap());
        // one storey is another picture; with no boxes at all there is still one
        assert_ne!(render_bare(40, "no", &View { storey: Some(276), ..boxed(None, None) }, 800).png().unwrap(), plain.png().unwrap());
        assert_eq!(render_bare(40, "no", &View::default(), 400).w, 400);
    }
}
