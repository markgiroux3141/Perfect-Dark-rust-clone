//! The side view: a section through the level, height upwards (docs/AUTOLEVEL.md, "Blank levels",
//! piece 7). New here: the Python has none.
//!
//! A section is a line on the plan, along x (at one row of cells, `y = at`) or along y (at one
//! column, `x = at`). The picture is in cells across and cells up (`z / UNITS`, so a storey is
//! about two and a third cells and the rooms keep their true proportions): the storeys' floor
//! lines labelled up the side, the rooms the line crosses drawn solid in their floor's colour
//! (with the room tool, the room boxes as the recipe places them, the one selected with grips on
//! its top, bottom and ends), the rooms it misses as faint outlines behind, and of the level the
//! decks and catwalks it cuts, the ramps as slopes, the corridors and vents at their heights.
//!
//! The same `Prim`s as the plan, so the window's canvas and the snapshot's rasteriser draw it as
//! they draw the plan. `hit` says what of the boxes a place shows, and `drag` what a drag of it
//! is: a move along the section (the body dragged across), a floor (dragged up or down: a `pin`),
//! a height (its top or bottom edge: `Change::Height`), or a side pulled (an end).

use crate::planview::{colour, Boxes, Key, Picture, Prim, Rgba, Swatch, P};
use autolevel::boxes::{dragged, Change, Part};
use autolevel::cells::{Cells, Rect};
use autolevel::level::Level;
use autolevel::scatter::{Lot, STOREY};
use autolevel::storeys::{snap_top, CEILINGS, LEVELS};

/// A cell, in units: the router's grid is 3 m, twice the furnisher's 59-unit half cell.
pub const UNITS: f32 = 118.0;
/// The height a corridor is drawn, and a vent's, in units.
const CORRIDOR: i32 = STOREY;
const VENT: i32 = 59;
/// Cells left of the level for the floor lines' labels.
const MARGIN: f32 = 7.0;

/// Which way the section runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Along x, through the row of cells `y = at`: west on the left.
    X,
    /// Along y, through the column of cells `x = at`: south on the left.
    Y,
}

/// A line through the plan to cut the level along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    pub axis: Axis,
    /// The row (along x) or column (along y) of cells it cuts through.
    pub at: i32,
}

impl Section {
    /// The section along `axis` through the middle of the rectangle `r`.
    pub fn through(axis: Axis, r: Rect) -> Section {
        let at = match axis {
            Axis::X => (r.1 + r.3 - 1).div_euclid(2),
            Axis::Y => (r.0 + r.2 - 1).div_euclid(2),
        };
        Section { axis, at }
    }

    /// The other way, through the same place `r`'s middle is (or the same number, with none).
    pub fn turned(self, r: Option<Rect>) -> Section {
        let axis = if self.axis == Axis::X { Axis::Y } else { Axis::X };
        r.map_or(Section { axis, at: self.at }, |r| Section::through(axis, r))
    }

    /// Does it cut through the rectangle `r` (cells)?
    pub fn crosses(&self, r: Rect) -> bool {
        match self.axis {
            Axis::X => r.1 <= self.at && self.at < r.3,
            Axis::Y => r.0 <= self.at && self.at < r.2,
        }
    }

    /// `r`'s extent along the section, in cells.
    pub fn span(&self, r: Rect) -> (i32, i32) {
        match self.axis {
            Axis::X => (r.0, r.2),
            Axis::Y => (r.1, r.3),
        }
    }

    /// A cell's place along the section, and whether it is on it (cells of the router's grid).
    fn on(&self, (x, y): (i32, i32)) -> Option<i32> {
        match self.axis {
            Axis::X => (y == self.at).then_some(x),
            Axis::Y => (x == self.at).then_some(y),
        }
    }

    /// A half cell's place along the section, in half cells, if it is on it.
    fn on_half(&self, (x, y): (i32, i32)) -> Option<i32> {
        match self.axis {
            Axis::X => (y.div_euclid(2) == self.at).then_some(x),
            Axis::Y => (x.div_euclid(2) == self.at).then_some(y),
        }
    }

    /// The part of a box a pull of its end `s` (-1 the low end, 1 the high) is on the plan.
    pub fn end(&self, s: i8) -> Part {
        match self.axis {
            Axis::X => Part::Edge(s, 0),
            Axis::Y => Part::Edge(0, s),
        }
    }

    /// Read from the command line's spelling: `x@Y` (along x, at the row y = Y) or `y@X`.
    pub fn parse(text: &str) -> Result<Section, String> {
        let bad = || format!("a section is x@Y (along x, at the row of cells y = Y) or y@X, not {text:?}");
        let (axis, at) = text.split_once('@').ok_or_else(bad)?;
        let axis = match axis {
            "x" | "X" => Axis::X,
            "y" | "Y" => Axis::Y,
            _ => return Err(bad()),
        };
        Ok(Section { axis, at: at.trim().parse().map_err(|_| bad())? })
    }

    /// In words: "along x, through y = 12".
    pub fn words(&self) -> String {
        match self.axis {
            Axis::X => format!("along x (west to east), through the row y = {}", self.at),
            Axis::Y => format!("along y (south to north), through the column x = {}", self.at),
        }
    }
}

/// A height in units, in the picture's cells up.
pub fn up(z: i32) -> f32 {
    z as f32 / UNITS
}

/// What of the boxes a place in the side view shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidePart {
    /// The box itself: a drag across moves it along the section, up or down puts its floor on
    /// another storey.
    Body,
    /// Its top edge (a drag sets its top) or its bottom edge (its floor, the top kept).
    Top,
    Bottom,
    /// One of its ends (-1 the low one, 1 the high): a drag pulls that side of the box.
    End(i8),
}

/// A box hit in the side view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideHit {
    pub key: String,
    pub part: SidePart,
}

/// A lot's box in the picture: (left, bottom, right, top) in cells.
fn frame(s: &Section, a: &Lot) -> (f32, f32, f32, f32) {
    let (u0, u1) = s.span(a.rect());
    (u0 as f32, up(a.floor), u1 as f32, up(a.top))
}

/// What of the boxes the section cuts is at `at` (cells across, cells up): the selected box's
/// top, bottom or an end if `at` is within `grip` cells of it, or else the body of the smallest
/// box there (a pod over its hall).
pub fn hit(lots: &[Lot], s: &Section, at: P, selected: Option<&str>, grip: f32) -> Option<SideHit> {
    if let Some(a) = selected.and_then(|key| lots.iter().find(|a| a.key == key)).filter(|a| s.crosses(a.rect())) {
        let (l, b, r, t) = frame(s, a);
        // (inside the box an edge's hold is a third of the way across at most)
        let (gu, gv) = (grip.min((r - l) / 3.0), grip.min((t - b) / 3.0));
        let across = l - grip <= at.0 && at.0 <= r + grip;
        let along = b - grip <= at.1 && at.1 <= t + grip;
        if across && (at.1 - t).abs() <= gv.max(grip * 0.5) && at.1 >= t - gv {
            return Some(SideHit { key: a.key.clone(), part: SidePart::Top });
        }
        if across && (at.1 - b).abs() <= gv.max(grip * 0.5) && at.1 <= b + gv {
            return Some(SideHit { key: a.key.clone(), part: SidePart::Bottom });
        }
        if along && at.0 <= l + gu && at.0 >= l - grip {
            return Some(SideHit { key: a.key.clone(), part: SidePart::End(-1) });
        }
        if along && at.0 >= r - gu && at.0 <= r + grip {
            return Some(SideHit { key: a.key.clone(), part: SidePart::End(1) });
        }
    }
    let inside = |a: &&Lot| {
        let (l, b, r, t) = frame(s, a);
        s.crosses(a.rect()) && l <= at.0 && at.0 <= r && b <= at.1 && at.1 <= t
    };
    let area = |a: &Lot| {
        let (l, b, r, t) = frame(s, a);
        (r - l) * (t - b)
    };
    let best = lots.iter().filter(inside).min_by(|a, b| area(a).total_cmp(&area(b)))?;
    Some(SideHit { key: best.key.clone(), part: SidePart::Body })
}

/// The storey floor nearest a height (units).
fn nearest_floor(z: f32) -> i32 {
    *LEVELS.iter().min_by(|a, b| (**a as f32 - z).abs().total_cmp(&(**b as f32 - z).abs())).expect("storeys")
}

/// The ceiling line nearest a height (units).
fn nearest_ceiling(z: f32) -> i32 {
    *CEILINGS.iter().min_by(|a, b| (**a as f32 - z).abs().total_cmp(&(**b as f32 - z).abs())).expect("storeys")
}

/// The change a drag of `part` of the box `lot` from `from` to `to` (cells across, cells up) is,
/// in whole cells across and whole storeys up. A drag of the body goes the way it has gone
/// furthest: across is a move along the section, up or down a `pin` to another storey. None if
/// it changes nothing.
pub fn drag(lot: &Lot, s: &Section, part: SidePart, from: P, to: P) -> Option<Change> {
    let (du, dv) = (to.0 - from.0, to.1 - from.1);
    let key = || lot.key.clone();
    let top = snap_top(lot.top);
    let change = match part {
        SidePart::Body if du.abs() >= dv.abs() => {
            let by = du.round() as i32;
            let by = match s.axis {
                Axis::X => (by, 0),
                Axis::Y => (0, by),
            };
            let r = dragged(lot.rect(), Part::Body, by);
            Change::Move { room: key(), to: (r.0, r.1) }
        }
        SidePart::Body => Change::Pin { room: key(), floor: nearest_floor(lot.floor as f32 + dv * UNITS) },
        SidePart::Top => {
            // (never under a storey tall)
            let least = CEILINGS[LEVELS.iter().position(|&z| z == lot.floor).unwrap_or(0)];
            Change::Height { room: key(), floor: lot.floor, top: nearest_ceiling(top as f32 + dv * UNITS).max(least) }
        }
        SidePart::Bottom => {
            let most = *LEVELS.iter().filter(|&&z| z < top).max().unwrap_or(&lot.floor);
            Change::Height { room: key(), floor: nearest_floor(lot.floor as f32 + dv * UNITS).min(most), top }
        }
        SidePart::End(e) => {
            let by = du.round() as i32;
            let by = match s.axis {
                Axis::X => (by, 0),
                Axis::Y => (0, by),
            };
            Change::Rect { room: key(), rect: dragged(lot.rect(), s.end(e), by) }
        }
    };
    let same = match &change {
        Change::Move { to, .. } => *to == (lot.x0, lot.y0),
        Change::Pin { floor, .. } => *floor == lot.floor,
        Change::Height { floor, top: t, .. } => *floor == lot.floor && *t == top,
        Change::Rect { rect, .. } => *rect == lot.rect(),
        _ => false,
    };
    (!same).then_some(change)
}

/// What to show in the side view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SideView {
    pub section: Option<Section>,
    /// The room boxes (the room tool is in hand), drawn over the level.
    pub boxes: Option<Boxes>,
    /// Pixels to a cell where it will be drawn, as near as is known: which names fit.
    pub scale: f32,
}

struct Pen<'a> {
    s: Section,
    out: Vec<Prim>,
    lv: Option<&'a Level>,
    /// A grip's radius, in cells: six pixels at the view's scale.
    grip: f32,
}

/// A colour with its alpha set.
fn alpha(c: Rgba, a: u8) -> Rgba {
    [c[0], c[1], c[2], a]
}

/// The colour of the storey whose floor is `z` (or the nearest one under it).
fn storey_colour(z: i32) -> Rgba {
    let n = LEVELS.iter().rposition(|&l| l <= z).unwrap_or(0);
    colour::STOREY[n]
}

impl Pen<'_> {
    fn rect(&mut self, (l, b, r, t): (f32, f32, f32, f32), fill: Rgba) {
        self.out.push(Prim::Poly { pts: vec![(l, b), (r, b), (r, t), (l, t)], fill });
    }

    fn outline(&mut self, (l, b, r, t): (f32, f32, f32, f32), col: Rgba, width_px: f32) {
        for (a, z) in [((l, b), (r, b)), ((r, b), (r, t)), ((r, t), (l, t)), ((l, t), (l, b))] {
            self.out.push(Prim::Line { a, b: z, col, width_px });
        }
    }

    fn line(&mut self, a: P, b: P, col: Rgba, width_px: f32) {
        self.out.push(Prim::Line { a, b, col, width_px });
    }

    fn text(&mut self, at: P, text: String, col: Rgba, size_px: f32, centred: bool) {
        self.out.push(Prim::Text { at, text, col, size_px, centred });
    }

    /// The floor lines (labelled with their heights) and ceiling lines, and the cells across.
    fn grid(&mut self, lo: P, hi: P) {
        for x in lo.0.ceil() as i32..=hi.0.floor() as i32 {
            if x.rem_euclid(4) == 0 {
                self.line((x as f32, lo.1), (x as f32, hi.1), colour::GRID, 1.0);
            }
        }
        for (n, (&z, &c)) in LEVELS.iter().zip(&CEILINGS).enumerate() {
            if up(z) < lo.1 || up(z) > hi.1 {
                continue;
            }
            self.line((lo.0, up(z)), (hi.0, up(z)), colour::GRID_FOURTH, 1.0);
            self.line((lo.0 + MARGIN - 0.5, up(c)), (hi.0, up(c)), colour::GRID, 1.0);
            let name = match n {
                0 => "low road".to_string(),
                1 => "ground".to_string(),
                n => format!("floor {}", n - 1),
            };
            self.text((lo.0 + 0.4, up(z) + 0.55), format!("{name}  {z}"), alpha(colour::STOREY[n], 255), 8.0, false);
        }
    }

    /// Runs of consecutive places along the section, from places in any order.
    fn runs(mut at: Vec<i32>) -> Vec<(i32, i32)> {
        at.sort_unstable();
        at.dedup();
        let mut out: Vec<(i32, i32)> = Vec::new();
        for u in at {
            match out.last_mut() {
                Some(run) if run.1 == u => run.1 = u + 1,
                _ => out.push((u, u + 1)),
            }
        }
        out
    }

    /// Router cells on the section, as runs.
    fn cut(&self, cells: &Cells) -> Vec<(i32, i32)> {
        Self::runs(cells.iter().filter_map(|&c| self.s.on(c)).collect())
    }

    /// Half cells on the section, as runs (in half cells).
    fn cut_half(&self, cells: &Cells) -> Vec<(i32, i32)> {
        Self::runs(cells.iter().filter_map(|&c| self.s.on_half(c)).collect())
    }

    /// A room: solid where the section crosses it, a faint outline behind where it does not.
    fn room(&mut self, key: &str, rect: Rect, floor: i32, top: i32, pod: bool) {
        let (u0, u1) = self.s.span(rect);
        let f = (u0 as f32, up(floor), u1 as f32, up(top));
        let c = storey_colour(floor);
        if self.s.crosses(rect) {
            self.rect(f, alpha(c, if pod { 90 } else { 60 }));
            self.outline(f, alpha(c, 230), if pod { 1.5 } else { 2.0 });
            let _ = key;
        } else {
            self.outline(f, alpha(c, 40), 1.0);
        }
    }

    /// What the level has on the section: decks and catwalks, ramps, corridors and vents.
    fn level(&mut self, lv: &Level) {
        for (key, it) in &lv.interiors {
            if !self.s.crosses(lv.space(key).rect()) {
                continue;
            }
            for (&z, cells) in &it.deck {
                for (h0, h1) in self.cut_half(cells) {
                    self.line((h0 as f32 / 2.0, up(z)), (h1 as f32 / 2.0, up(z)), alpha(storey_colour(z), 255), 3.0);
                }
            }
            for ramp in &it.ramps {
                let r = ramp.rect;
                let (cross, (h0, h1), along) = match self.s.axis {
                    Axis::X => (r.1.div_euclid(2) <= self.s.at && self.s.at <= (r.3 - 1).div_euclid(2), (r.0, r.2), ramp.d % 2 == 0),
                    Axis::Y => (r.0.div_euclid(2) <= self.s.at && self.s.at <= (r.2 - 1).div_euclid(2), (r.1, r.3), ramp.d % 2 == 1),
                };
                if !cross {
                    continue;
                }
                let (u0, u1) = (h0 as f32 / 2.0, h1 as f32 / 2.0);
                if along {
                    // (it climbs towards the high end for d = 0 or 1, the low end for 2 or 3)
                    let (foot, head) = if ramp.d < 2 { (u0, u1) } else { (u1, u0) };
                    self.line((foot, up(ramp.lo)), (head, up(ramp.hi)), colour::RAMP_ARROW, 3.0);
                } else {
                    // (crossed: the ramp is a slope across the cut, its width at both heights)
                    self.outline((u0, up(ramp.lo), u1, up(ramp.hi)), alpha(colour::RAMP_ARROW, 150), 1.0);
                    self.line((u0, up(ramp.lo)), (u1, up(ramp.hi)), alpha(colour::RAMP_ARROW, 150), 1.0);
                }
            }
        }
        for route in lv.net.routes() {
            for pc in &route.pieces {
                let vent = pc.kind == "vent";
                for (z, cells) in &pc.cells {
                    for (u0, u1) in self.cut(cells) {
                        let tall = if vent { VENT } else { CORRIDOR };
                        let f = (u0 as f32, up(*z), u1 as f32, up(z + tall));
                        let c = if vent { colour::VENT } else { storey_colour(*z) };
                        self.rect(f, alpha(c, 45));
                        self.outline(f, alpha(c, 170), 1.0);
                        self.line((f.0, f.1), (f.2, f.1), alpha(c, 255), 2.5);
                    }
                }
                let sec = self.s;
                let near = |p: &(f64, f64, i32)| {
                    let across = match sec.axis {
                        Axis::X => p.1,
                        Axis::Y => p.0,
                    };
                    (across - (sec.at as f64 + 0.5)).abs() <= 1.0
                };
                let along = |p: &(f64, f64, i32)| match sec.axis {
                    Axis::X => p.0 as f32,
                    Axis::Y => p.1 as f32,
                };
                if let Some((foot, head)) = &pc.ramp {
                    if near(foot) || near(head) {
                        self.line((along(foot), up(foot.2)), (along(head), up(head.2)), colour::RAMP_ARROW, 3.0);
                    }
                }
                for ((foot, head), _) in &pc.ducts {
                    if near(foot) || near(head) {
                        self.line((along(foot), up(foot.2)), (along(head), up(head.2)), colour::VENT, 2.0);
                    }
                }
            }
        }
    }

    /// The room boxes as the recipe places them, the selected one with its grips and a refusal
    /// beside it.
    fn boxes(&mut self, b: &Boxes) {
        let sec = self.s;
        for a in b.lots.iter().filter(|a| !sec.crosses(a.rect())) {
            self.room(&a.key, a.rect(), a.floor, a.top, !a.host.is_empty());
        }
        // (halls first, their pods over them)
        let crossed: Vec<&Lot> = b.lots.iter().filter(|a| sec.crosses(a.rect())).collect();
        for a in crossed.iter().filter(|a| a.host.is_empty()).chain(crossed.iter().filter(|a| !a.host.is_empty())) {
            let f = frame(&self.s, a);
            let c = storey_colour(a.floor);
            // (over the level, the box is an edge and a tint: the level's own lines show through)
            self.rect(f, alpha(c, if self.lv.is_some() { 30 } else { 70 }));
            self.outline(f, alpha(c, 240), 2.0);
            let fits = (f.2 - f.0) >= 3.0;
            if fits {
                self.text(((f.0 + f.2) / 2.0, f.3 - 0.6), a.key.clone(), colour::NAME, 8.0, true);
            }
        }
        let Some(a) = b.selected.as_deref().and_then(|key| b.lots.iter().find(|a| a.key == key)) else { return };
        if !self.s.crosses(a.rect()) {
            return;
        }
        let f = frame(&self.s, a);
        let edge = if b.refused.is_some() { colour::DEAD_END } else { colour::PICK };
        self.outline(f, edge, 3.5);
        // grips: the middle of the top and the bottom, the middle of each end
        let (mu, mv) = ((f.0 + f.2) / 2.0, (f.1 + f.3) / 2.0);
        for c in [(mu, f.3), (mu, f.1), (f.0, mv), (f.2, mv)] {
            self.out.push(Prim::Circle { c, r: self.grip, col: edge, filled: true });
        }
        let mut lines = vec![format!("{} ({}): {} to {}", a.key, a.kind, a.floor, snap_top(a.top))];
        if let Some(why) = &b.refused {
            lines.extend(crate::planview::wrap(why, 46));
        }
        for (n, line) in lines.into_iter().enumerate() {
            self.text((f.2 + 0.8, f.3 - 0.5 - n as f32 * 0.9), line, if n == 0 { colour::KEY } else { colour::DEAD_END }, 8.0, false);
        }
    }
}

/// The bounds of the side view in cells across and up: the level's (or the boxes') along the
/// section, from under the lowest floor to over the highest top, with the labels' margin.
pub fn bounds(lv: Option<&Level>, view: &SideView) -> (P, P) {
    let Some(s) = view.section else { return ((0.0, 0.0), (1.0, 1.0)) };
    let mut spans: Vec<(i32, i32, i32, i32)> = Vec::new();
    if let Some(lv) = lv {
        spans.extend(lv.spaces().iter().map(|a| {
            let (u0, u1) = s.span(a.rect());
            (u0, u1, a.floor, a.top)
        }));
    }
    for a in view.boxes.iter().flat_map(|b| &b.lots) {
        let (u0, u1) = s.span(a.rect());
        spans.push((u0, u1, a.floor, a.top));
    }
    if let Some((w, h)) = view.boxes.as_ref().and_then(|b| b.site) {
        let n = if s.axis == Axis::X { w } else { h };
        spans.push((0, n, LEVELS[1], CEILINGS[1]));
    }
    if spans.is_empty() {
        return ((0.0, 0.0), (1.0, 1.0));
    }
    let u0 = spans.iter().map(|a| a.0).min().unwrap() as f32;
    let u1 = spans.iter().map(|a| a.1).max().unwrap() as f32;
    let z0 = spans.iter().map(|a| a.2).min().unwrap().min(LEVELS[1]);
    let z1 = spans.iter().map(|a| a.3).max().unwrap();
    ((u0 - MARGIN, up(z0) - 1.0), (u1 + 6.0, up(z1) + 1.5))
}

/// The side view of a level (or, with none, of the room boxes alone) along `view.section`.
pub fn picture(lv: Option<&Level>, view: &SideView) -> Picture {
    let (lo, hi) = bounds(lv, view);
    let Some(s) = view.section else { return Picture { prims: Vec::new(), lo, hi, legend: Vec::new() } };
    let mut pen = Pen { s, out: Vec::new(), lv, grip: 6.0 / view.scale.max(1.0) };
    pen.grid(lo, hi);
    if let Some(lv) = lv {
        if view.boxes.is_none() {
            // (rooms behind first, then those cut, halls under their pods)
            let mut spaces: Vec<_> = lv.spaces().iter().collect();
            spaces.sort_by_key(|a| (s.crosses(a.rect()), !a.host.is_empty()));
            for a in spaces {
                pen.room(&a.key, a.rect(), a.floor, a.top, !a.host.is_empty());
            }
        }
        pen.level(lv);
        if view.boxes.is_none() {
            for a in lv.spaces().iter().filter(|a| s.crosses(a.rect())) {
                let (u0, u1) = s.span(a.rect());
                if u1 - u0 >= 3 {
                    pen.text(((u0 + u1) as f32 / 2.0, up(a.top) - 0.6), a.key.clone(), colour::NAME, 8.0, true);
                }
            }
        }
    }
    if let Some(b) = &view.boxes {
        pen.boxes(b);
    }
    let legend = vec![
        Key { swatch: Swatch::Fill, col: alpha(colour::STOREY[1], 120), text: "a room the section cuts, in its floor's colour".to_string() },
        Key { swatch: Swatch::Ring, col: alpha(colour::STOREY[1], 80), text: "a room in front or behind".to_string() },
        Key { swatch: Swatch::Line, col: colour::STOREY[2], text: "a deck or catwalk".to_string() },
        Key { swatch: Swatch::Line, col: colour::RAMP_ARROW, text: "a ramp".to_string() },
        Key { swatch: Swatch::Line, col: colour::VENT, text: "a vent".to_string() },
        Key { swatch: Swatch::Ring, col: colour::PICK, text: "selected: drag it across or up, or its top, bottom or ends".to_string() },
    ];
    Picture { prims: pen.out, lo, hi, legend }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    fn lot(key: &str, rect: Rect, floor: i32, top: i32) -> Lot {
        Lot { key: key.into(), kind: "medium", x0: rect.0, y0: rect.1, x1: rect.2, y1: rect.3, floor, top, host: String::new() }
    }

    #[test]
    fn a_section_is_read_and_crosses_what_it_cuts() {
        let s = Section::parse("x@12").unwrap();
        assert_eq!(s, Section { axis: Axis::X, at: 12 });
        assert_eq!(Section::parse("y@3").unwrap(), Section { axis: Axis::Y, at: 3 });
        assert!(Section::parse("z@3").is_err() && Section::parse("x12").is_err() && Section::parse("x@a").is_err());
        assert!(s.crosses((0, 12, 4, 13)) && !s.crosses((0, 13, 4, 20)) && !s.crosses((0, 5, 4, 12)));
        assert_eq!(s.span((3, 0, 9, 20)), (3, 9));
        assert_eq!(Section::through(Axis::X, (10, 20, 16, 25)), Section { axis: Axis::X, at: 22 });
        assert_eq!(Section::through(Axis::Y, (10, 20, 16, 25)), Section { axis: Axis::Y, at: 12 });
        assert_eq!(s.turned(Some((10, 20, 16, 25))), Section { axis: Axis::Y, at: 12 });
    }

    #[test]
    fn the_selected_box_is_held_by_its_top_bottom_and_ends_and_others_by_their_body() {
        let s = Section { axis: Axis::X, at: 5 };
        let hall = lot("hall", (0, 0, 20, 10), 0, 787);
        let pod = Lot { host: "hall".into(), ..lot("pod", (2, 3, 6, 7), 276, 512) };
        let behind = lot("behind", (30, 20, 40, 30), 0, 236);
        let lots = [hall, pod, behind];
        // a pod over its hall: the smaller box wins the body
        assert_eq!(hit(&lots, &s, (4.0, up(400)), None, 0.5).map(|h| h.key), Some("pod".into()));
        assert_eq!(hit(&lots, &s, (12.0, up(400)), None, 0.5).map(|h| h.key), Some("hall".into()));
        // a room the section misses is not there to hold
        assert_eq!(hit(&lots, &s, (35.0, up(100)), None, 0.5), None);
        let part = |at: P| hit(&lots, &s, at, Some("hall"), 0.5).map(|h| h.part);
        assert_eq!(part((12.0, up(787))), Some(SidePart::Top));
        assert_eq!(part((12.0, 0.0)), Some(SidePart::Bottom));
        assert_eq!(part((0.1, up(600))), Some(SidePart::End(-1)));
        assert_eq!(part((19.9, up(600))), Some(SidePart::End(1)));
        assert_eq!(part((12.0, up(600))), Some(SidePart::Body));
    }

    #[test]
    fn a_drag_in_the_side_view_is_a_move_a_pin_a_height_or_a_side_pulled() {
        let s = Section { axis: Axis::Y, at: 3 };
        let a = lot("a", (2, 10, 8, 16), 276, 787);
        let at = (13.0, up(500));
        // across: along the section (y), whole cells; up: a storey, as a pin
        assert_eq!(drag(&a, &s, SidePart::Body, at, (16.2, up(520))), Some(Change::Move { room: "a".into(), to: (2, 13) }));
        assert_eq!(drag(&a, &s, SidePart::Body, at, (13.1, up(790))), Some(Change::Pin { room: "a".into(), floor: 551 }));
        assert_eq!(drag(&a, &s, SidePart::Body, at, (13.1, up(530))), None);
        // its top up a storey, its bottom down one (the top kept)
        assert_eq!(drag(&a, &s, SidePart::Top, at, (13.0, up(770))), Some(Change::Height { room: "a".into(), floor: 276, top: 1063 }));
        assert_eq!(drag(&a, &s, SidePart::Bottom, at, (13.0, up(230))), Some(Change::Height { room: "a".into(), floor: 0, top: 787 }));
        // never under a storey tall
        assert_eq!(drag(&a, &s, SidePart::Top, at, (13.0, up(-900))), Some(Change::Height { room: "a".into(), floor: 276, top: 512 }));
        assert_eq!(drag(&a, &s, SidePart::Bottom, at, (13.0, up(5000))), Some(Change::Height { room: "a".into(), floor: 551, top: 787 }));
        // an end pulled: the box along y
        assert_eq!(drag(&a, &s, SidePart::End(1), at, (15.0, at.1)), Some(Change::Rect { room: "a".into(), rect: (2, 10, 8, 18) }));
    }

    #[test]
    fn the_side_view_of_seed_40_cuts_its_rooms_decks_and_corridors() {
        let made = fixture::seed40();
        let vast = made.level.space("vast").rect();
        let s = Section::through(Axis::X, vast);
        let view = SideView { section: Some(s), boxes: None, scale: 8.0 };
        let pic = picture(Some(&made.level), &view);
        let (lo, hi) = (pic.lo, pic.hi);
        assert!(lo.0 < vast.0 as f32 && hi.0 > vast.2 as f32, "the hall is in the bounds: {lo:?} {hi:?}");
        let texts: Vec<&str> = pic.prims.iter().filter_map(|p| if let Prim::Text { text, .. } = p { Some(text.as_str()) } else { None }).collect();
        assert!(texts.contains(&"vast"), "the hall is named: {texts:?}");
        assert!(texts.iter().any(|t| t.starts_with("ground")), "the floor lines are labelled");
        // its decks are drawn as thick lines at their storeys
        let decks = pic.prims.iter().filter(|p| matches!(p, Prim::Line { width_px, .. } if *width_px == 3.0)).count();
        assert!(decks > 0, "decks or ramps on the section");
        // the same view twice is the same picture
        assert_eq!(picture(Some(&made.level), &view), pic);
    }
}
