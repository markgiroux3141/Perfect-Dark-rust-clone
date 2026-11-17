//! A plan drawn top down as an SVG, a panel for each band of floors: `plan.svg`. From
//! `write_svg` in `ge_pd_tools/plan.py`, as `scripts/autolevel.py` calls it (`PANELS`, `SCALE`,
//! the level's numbers as the title).
//!
//! Rooms are coloured by floor level (faint where a panel shows only their air), solids are
//! drawn darker, or as floor where their tops are walked on, ramps are arrows pointing uphill,
//! railings are dashed, and every opening between two rooms is a red line. Numbers are written
//! as Python writes them (`round(x, 1)`, and a float's shortest text), so the file is the
//! Python's element for element. Not ported: sightlines and spots, which a generated plan has none of.

use autolevel::plan::{cells, Plan, Point3, MODULE};
use autolevel::py;

/// A panel: its label, and the lowest and highest floor it shows.
pub type Panel = (&'static str, i32, i32);

/// The panels `scripts/autolevel.py` draws.
pub const PANELS: [Panel; 5] =
    [("Low road (-276)", -400, -1), ("Ground (0)", 0, 275), ("Upper (276)", 276, 550), ("Top (551, 827)", 551, 1100), ("High (1102 and up)", 1101, 2000)];
/// Pixels to a unit, as `scripts/autolevel.py` draws it.
pub const SCALE: f64 = 0.085;
/// The margin round each panel, in pixels.
pub const PAD: i32 = 36;

/// The colour of the floor level nearest `z` (the first of two as near).
fn level_colour(z: i32) -> &'static str {
    const TABLE: [(i32, &str); 5] = [(-276, "#2d3a8c"), (-118, "#2f7fa6"), (0, "#2fae8f"), (276, "#86c64a"), (551, "#d9ae6a")];
    py::first_min(TABLE, |t| (t.0 - z).abs()).expect("the table has rows").1
}

/// Do the heights z0 to z1 meet the band zb0 to zb1 (all inclusive)?
fn band_of(z0: i32, z1: i32, zb0: i32, zb1: i32) -> bool {
    (z1 + 1).min(zb1 + 1) - z0.max(zb0) > 0
}

/// A float as Python's `str` writes it: the shortest text that reads back the same, "36.0" for a whole number.
fn num(x: f64) -> String {
    format!("{x:?}")
}

/// Text with what XML reads as markup escaped.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The plan as an SVG. `scale` is pixels to a unit; `title` goes under the panels (empty: none).
pub fn svg(plan: &Plan, panels: &[Panel], scale: f64, title: &str) -> String {
    let fold = |pick: fn(&autolevel::plan::Room) -> Point3, least: bool| {
        let pts: Vec<Point3> = plan.rooms.iter().map(pick).collect();
        let one = |get: fn(&Point3) -> i32| pts.iter().map(get).reduce(|a, b| if least { a.min(b) } else { a.max(b) }).unwrap_or(0);
        (one(|p| p.0), one(|p| p.1), one(|p| p.2))
    };
    let (lo, hi) = (fold(|r| r.lo(), true), fold(|r| r.hi(), false));
    let pad = PAD as f64;
    let w1 = (hi.0 - lo.0) as f64 * scale + 2.0 * pad;
    let h1 = (hi.1 - lo.1) as f64 * scale + 2.0 * pad + 24.0;
    let (w, h) = (w1 * panels.len() as f64, h1 + 70.0);
    let mut out = vec![
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.0} {h:.0}\" font-family=\"Consolas, monospace\">"
        ),
        "<rect width=\"100%\" height=\"100%\" fill=\"#16181d\"/>".to_string(),
        "<defs><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"8\" refY=\"5\" markerWidth=\"4\" markerHeight=\"4\" orient=\"auto-start-reverse\">\
         <path d=\"M0,0 L10,5 L0,10 z\" fill=\"#ffe066\"/></marker></defs>"
            .to_string(),
    ];
    if !title.is_empty() {
        out.push(format!("<text x=\"{PAD}\" y=\"{}\" fill=\"#ddd\" font-size=\"15\">{}</text>", num(h - 40.0), escape(title)));
    }
    let (contacts, _) = plan.contacts();

    for (pi, &(label, zb0, zb1)) in panels.iter().enumerate() {
        let ox = pi as f64 * w1;
        let band = |z0: i32, z1: i32| band_of(z0, z1, zb0, zb1);
        // a point of the plan (units, y up the page) in pixels, to a tenth
        let xy = |x: f64, y: f64| (py::round_to(ox + (x - lo.0 as f64) * scale + pad, 1), py::round_to((hi.1 as f64 - y) * scale + pad + 24.0, 1));
        let at = |p: Point3| xy(p.0 as f64, p.1 as f64);
        let rect = |a: Point3, b: Point3, attrs: &str| {
            let ((x0, y1), (x1, y0)) = (at(a), at(b));
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" {attrs}/>",
                num(x0),
                num(y0),
                num(py::round_to(x1 - x0, 1)),
                num(py::round_to(y1 - y0, 1))
            )
        };
        let line = |a: (f64, f64), b: (f64, f64), attrs: &str| {
            format!("<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" {attrs}/>", num(a.0), num(a.1), num(b.0), num(b.1))
        };

        out.push(format!("<text x=\"{}\" y=\"{PAD}\" fill=\"#eee\" font-size=\"14\">{label}</text>", num(ox + pad)));
        // the grid: a line a cell, every fourth one stronger
        let faint = |n: i32| if n % 4 == 0 { "0.13" } else { "0.05" };
        for i in 0..((hi.0 - lo.0) as f64 / MODULE) as i32 + 2 {
            let x = num(xy((lo.0 + cells(i as f64)) as f64, 0.0).0);
            out.push(format!("<line x1=\"{x}\" y1=\"{}\" x2=\"{x}\" y2=\"{}\" stroke=\"#fff\" stroke-opacity=\"{}\"/>", PAD + 24, num(h1 - pad), faint(i)));
        }
        for j in 0..((hi.1 - lo.1) as f64 / MODULE) as i32 + 2 {
            let y = num(xy(0.0, (lo.1 + cells(j as f64)) as f64).1);
            out.push(format!(
                "<line x1=\"{}\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" stroke=\"#fff\" stroke-opacity=\"{}\"/>",
                num(ox + pad),
                num(ox + w1 - pad),
                faint(j)
            ));
        }
        // the rooms with a floor in the band, and the halls whose air reaches up into it
        let shown: Vec<&autolevel::plan::Room> = plan
            .rooms
            .iter()
            .filter(|r| {
                let hall = matches!(r.kind, "hub" | "room") && zb0 >= 0;
                r.boxes.iter().any(|b| band(b.0 .2, b.0 .2) || (hall && band(b.0 .2, b.1 .2))) || band(r.floor, r.floor)
            })
            .collect();
        let mut low_first = shown.clone();
        low_first.sort_by_key(|r| r.floor);
        for r in low_first {
            let own = band(r.floor, r.floor);
            let fill = level_colour(if own { r.floor } else { zb0 });
            let op = if own { "0.95" } else { "0.35" };
            for b in &r.boxes {
                let bz = b.0 .2;
                let f = if band(bz, bz) { level_colour(bz) } else { fill };
                out.push(rect(b.0, b.1, &format!("fill=\"{f}\" fill-opacity=\"{op}\" stroke=\"#0b0c0f\" stroke-width=\"1\"")));
            }
            for &(s_lo, s_hi, tag) in &r.solids {
                let (top, base) = (s_hi.2, zb0.max(r.floor));
                if r.walkable.contains(&tag) && band(top, top) {
                    out.push(rect(s_lo, s_hi, &format!("fill=\"{}\" fill-opacity=\"0.95\" stroke=\"#e8e8e8\" stroke-width=\"0.6\"", level_colour(top))));
                } else if s_lo.2 <= base + 160 && s_hi.2 > base + 60 {
                    out.push(rect(s_lo, s_hi, "fill=\"#0b0c0f\" stroke=\"#777\" stroke-width=\"0.6\""));
                }
            }
            for rp in &r.ramps {
                if !(band(rp.start.2, rp.start.2) || band(rp.end.2, rp.end.2)) {
                    continue;
                }
                let (s, e) = if rp.start.2 < rp.end.2 { (rp.start, rp.end) } else { (rp.end, rp.start) };
                out.push(line(at(s), at(e), "stroke=\"#ffe066\" stroke-width=\"3\" marker-end=\"url(#arrow)\""));
            }
            for &(p0, p1, _) in &r.rails {
                if band(p0.2, p0.2) {
                    out.push(line(at(p0), at(p1), "stroke=\"#cfd8dc\" stroke-width=\"1.5\" stroke-dasharray=\"2 2\""));
                }
            }
        }
        // openings between rooms, at this band's own floors
        for c in &contacts {
            if c.axis == 2 || !band(c.lo.2 + 1, c.hi.2 - 1) {
                continue;
            }
            if !band_of(c.lo.2, c.lo.2, zb0 - 1, zb1) && c.lo.2 < zb0 {
                continue;
            }
            out.push(line(at(c.lo), at(c.hi), "stroke=\"#ff4d4d\" stroke-width=\"3\""));
        }
        // names, in the middle of each room's largest box
        for r in &shown {
            if !band(r.floor, r.floor) && r.kind != "hub" {
                continue;
            }
            let Some(b) = py::first_max(&r.boxes, |b| (b.1 .0 - b.0 .0) as i64 * (b.1 .1 - b.0 .1) as i64) else { continue };
            let (cx, cy) = xy((b.0 .0 + b.1 .0) as f64 / 2.0, (b.0 .1 + b.1 .1) as f64 / 2.0);
            out.push(format!(
                "<text x=\"{}\" y=\"{}\" fill=\"#fff\" font-size=\"{}\" text-anchor=\"middle\" stroke=\"#000\" stroke-width=\"2.5\" paint-order=\"stroke\">{:02X} {}</text>",
                num(cx),
                num(cy + 3.0),
                if r.kind == "hub" { 12 } else { 9 },
                r.room_id,
                escape(&r.key)
            ));
        }
    }
    for (i, (col, lab)) in [("#2d3a8c", "-276"), ("#2f7fa6", "-118"), ("#2fae8f", "0"), ("#86c64a", "276"), ("#d9ae6a", "551")].into_iter().enumerate() {
        let x = PAD + i as i32 * 80;
        out.push(format!(
            "<rect x=\"{x}\" y=\"{}\" width=\"14\" height=\"14\" fill=\"{col}\"/><text x=\"{}\" y=\"{}\" fill=\"#ccc\" font-size=\"11\">z {lab}</text>",
            num(h - 28.0),
            x + 20,
            num(h - 17.0)
        ));
    }
    out.push(format!(
        "<text x=\"{}\" y=\"{}\" fill=\"#ccc\" font-size=\"11\">red: openings   yellow arrow: ramp (uphill)   pink triangle: sniper perch   \
         blue dot: camping spot   dashes: sightlines (red = deliberate lanes)   dotted: railing   grid: 1 cell = 3 m = 118 units</text>",
        PAD + 420,
        num(h - 17.0)
    ));
    out.push("</svg>".to_string());
    out.join("\n")
}

/// How many of each element an SVG holds: (rects, lines, texts). For comparing two drawings.
pub fn counts(svg: &str) -> (usize, usize, usize) {
    (svg.matches("<rect ").count(), svg.matches("<line ").count(), svg.matches("<text ").count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use autolevel::plan::{box3, Ramp, Room};

    /// Two rooms side by side on the ground with a doorway between them, a deck in one, a crate
    /// in the other, and a ramp up to an upper room over the second.
    fn tiny() -> Plan {
        let mut p = Plan::new("tiny");
        let mut a = Room::new("hall", "The Hall", 0, vec![box3(0.0, 0.0, 4.0, 4.0, 0, 512)]);
        a.kind = "hub";
        a.solids.push(((0, 0, 261), (118, 472, 276), "grate"));
        a.walkable = vec!["grate"];
        a.rails.push(((118, 0, 276), (118, 472, 276), (1, 0, 0)));
        a.ramps.push(Ramp { start: (236, 0, 0), end: (236, 472, 276), width: 118, kind: "plate", open_sides: vec![] });
        p.room(a).unwrap();
        let mut b = Room::new("side", "Side", 0, vec![box3(4.0, 1.0, 6.0, 3.0, 0, 236)]);
        b.solids.push(((500, 150, 0), (560, 210, 118), "crate"));
        p.room(b).unwrap();
        p.room(Room::new("loft", "Loft", 276, vec![box3(4.0, 1.0, 6.0, 3.0, 276, 512)])).unwrap();
        p
    }

    /// The Python's `write_svg` drew the same plan with the same panels (`tests/tiny_plan.svg`):
    /// this must be that file, to the character.
    #[test]
    fn a_tiny_plan_is_drawn_as_the_python_draws_it() {
        let panels: [Panel; 2] = [("Ground (0)", 0, 275), ("Upper (276)", 276, 550)];
        let text = svg(&tiny(), &panels, 0.1, "tiny: 3 rooms");
        let python = include_str!("../tests/tiny_plan.svg").replace("\r\n", "\n");
        for (n, (mine, want)) in text.lines().zip(python.lines()).enumerate() {
            assert_eq!(mine, want, "line {}", n + 1);
        }
        assert_eq!(text, python);
        // the hall on the ground: its floor, bright and in the ground's colour, y down the page;
        // upstairs it is faint, as the air of a hall, and its deck is floor there
        assert!(text.contains(
            "<rect x=\"36.0\" y=\"60.0\" width=\"47.2\" height=\"47.2\" fill=\"#2fae8f\" fill-opacity=\"0.95\" stroke=\"#0b0c0f\" stroke-width=\"1\"/>"
        ));
        assert!(text.contains(
            "<rect x=\"178.9\" y=\"60.0\" width=\"47.2\" height=\"47.2\" fill=\"#86c64a\" fill-opacity=\"0.35\" stroke=\"#0b0c0f\" stroke-width=\"1\"/>"
        ));
        assert_eq!((text.matches("stroke=\"#e8e8e8\"").count(), text.matches("fill=\"#0b0c0f\" stroke=\"#777\"").count()), (1, 1));
        // the ramp is on both panels (its foot on one, its top on the other), the rail on one, and
        // each panel has the one opening on its own floor
        assert_eq!(
            (text.matches("marker-end=\"url(#arrow)\"").count(), text.matches("stroke-dasharray=\"2 2\"").count(), text.matches("stroke=\"#ff4d4d\"").count()),
            (2, 1, 2)
        );
        // names: both ground rooms downstairs; upstairs the loft, and the hall again as a hub
        assert_eq!((text.matches(" hall</text>").count(), text.matches(" side</text>").count(), text.matches(" loft</text>").count()), (2, 1, 1));
        assert_eq!(counts(&text), (12, 31, 13));
    }

    #[test]
    fn numbers_are_written_as_python_writes_them() {
        assert_eq!([36.0, 658.1, 0.0, 123.30000000000001].map(num), ["36.0", "658.1", "0.0", "123.30000000000001"]);
        assert_eq!(
            [-300, -200, -118, -60, 0, 137, 138, 413, 414, 2000].map(level_colour),
            ["#2d3a8c", "#2d3a8c", "#2f7fa6", "#2f7fa6", "#2fae8f", "#2fae8f", "#2fae8f", "#86c64a", "#d9ae6a", "#d9ae6a"]
        );
        assert!(band_of(0, 0, 0, 275) && band_of(275, 275, 0, 275) && !band_of(276, 276, 0, 275) && band_of(0, 512, 276, 550) && !band_of(-1, -1, 0, 275));
        assert_eq!(svg(&Plan::new("empty"), &[], 0.1, "").lines().count(), 10);
        assert_eq!(escape("a<b & c>d"), "a&lt;b &amp; c&gt;d");
    }
}
