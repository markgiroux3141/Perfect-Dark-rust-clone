//! How the window offers each of the recipe's options (`autolevel::recipe::OPTIONS`): a list to
//! choose from, a switch, or a slider over a range worth trying, with a line of help. Headless,
//! so it is tested without egui; `ui` draws it. The lists of words are the generator's own
//! (`level::STYLES`, `level::COVER`, `place::Scale`); the ranges are only the editor's: a recipe
//! may hold a value outside one (the command line takes any), and the slider then shows it at
//! its end without changing it.

use autolevel::level::{COVER, STYLES};

/// The control an option is set with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    /// One of these words.
    Choice(&'static [&'static str]),
    /// On or off.
    Switch,
    /// A whole number from `lo` to `hi`; `off` names the value 0 where 0 turns the thing off.
    Whole { lo: i64, hi: i64, off: bool },
    /// A number from `lo` to `hi` in steps of `step`.
    Share { lo: f64, hi: f64, step: f64 },
    /// The site: the scale's own, or cells wide by cells deep.
    Site,
}

/// The scales by name (`place::Scale::name`), in the order offered.
pub const SCALES: [&str; 3] = ["grand", "standard", "towering"];

/// A catwalk's widths, in cells.
pub const WALKWAYS: [&str; 2] = ["1", "2"];

/// The site's sliders: (fewest, most) cells wide, then deep.
pub const SITE_WIDE: (i32, i32) = (24, 200);
pub const SITE_DEEP: (i32, i32) = (24, 160);

/// How the option by this name (one of `OPTIONS`) is set, and a line saying what it does.
pub fn control(name: &str) -> (Control, &'static str) {
    use Control::*;
    match name {
        "style" => (Choice(&STYLES), "whose levels: Complex's halls and towers, or Felicity's building cut into rooms, with doors"),
        "washroom" => (Switch, "a Felicity level's washrooms have their stalls, toilets and basins"),
        "scale" => (Choice(&SCALES), "grand: a colossal hall, spiral towers six or seven storeys tall; standard: the first version's rooms"),
        "size" => (Share { lo: 0.5, hi: 4.0, step: 0.25 }, "how big: this many times the rooms (1 is about 29), on a site this many times the area"),
        "site" => (Site, "the ground the rooms are scattered on, in cells: the scale's own (grown with the size), or your own"),
        "cover" => {
            (Choice(&COVER), "what stands on the floors: pillars floor to ceiling, crates (low housings), nothing, or mixed (each room pillars or nothing)")
        }
        "clutter" => (Share { lo: 0.0, hi: 1.0, step: 0.05 }, "how much cover"),
        "walkway" => (Choice(&WALKWAYS), "catwalks, bridges and open ramps this many cells wide: 1 is a doorway's width"),
        "crates" => (Switch, "mixed rooms may take crates as well as pillars"),
        "shapes" => (Switch, "rooms may be L-shaped or have a cross wall; off keeps every room a plain rectangle"),
        "risers" => (Whole { lo: 0, hi: 8, off: false }, "enclosed ramp corridors (switchbacks) between rooms two or more storeys apart"),
        "second" => (Whole { lo: 0, hi: 30, off: true }, "a room with one door gets a second to a neighbour within this many cells"),
        "vents" => (Whole { lo: 0, hi: 12, off: false }, "crawl ducts, first for rooms left with one door, then as short cuts"),
        "windows" => (Whole { lo: 0, hi: 12, off: false }, "corridors that run past a hall open onto it, glazed or as a railed balcony"),
        "drops" => (Whole { lo: 0, hi: 8, off: false }, "holes in a room's floor into the room underneath"),
        "grates" => (Whole { lo: 0, hi: 10, off: false }, "walkable grates in a corridor's floor over a room, with a beam of light through"),
        "platforms" => {
            (Share { lo: 0.0, hi: 3.0, step: 0.25 }, "big platforms off a hall's catwalks into its empty air: one for each 100 cells of it, times this")
        }
        "galleries" => {
            (Whole { lo: 0, hi: 6, off: false }, "a towering hall's galleries: high storeys with no base given a catwalk of their own, reached from outside")
        }
        "dead_ends" => (Switch, "join up or cut short every dead end longer than the three limits below; off leaves them as they fall"),
        "dead_corridor" => (Whole { lo: 1, hi: 30, off: false }, "a room that goes nowhere may be this many cells of corridor off a main area"),
        "dead_walk" => (Whole { lo: 1, hi: 60, off: false }, "and this many cells' walk to its far end, ramps and decks too"),
        "dead_climb" => (Whole { lo: 1, hi: 6, off: false }, "and no climb of this many storeys may end on a deck with no room (1: no balconies either)"),
        "loops" => (Share { lo: 0.0, hi: 1.5, step: 0.05 }, "extra corridors that make loops, as a share of the number of rooms"),
        "shift" => (Whole { lo: 0, hi: 30, off: false }, "how far (cells) each room may be drawn in towards the middle; 0 leaves them where they fell"),
        "level_change" => (Share { lo: 0.0, hi: 40.0, step: 0.5 }, "how much further apart rooms with no storey in common count, when choosing what to join"),
        "tries" => (Whole { lo: 1, hi: 30, off: false }, "times a room is furnished, each with other random numbers, before the level is given up"),
        _ => (Switch, ""),
    }
}

/// Whether the option is read by a level of this style ("complex" reads all but `washroom`,
/// "felicity" only `washroom`; `style` itself always).
pub fn read_by(name: &str, style: &str) -> bool {
    match name {
        "style" => true,
        "washroom" => style == "felicity",
        _ => style == "complex",
    }
}

/// A number as `recipe::set_option` takes it and the panel shows it: no more decimals than it
/// needs (a slider's steps add float noise, as 0.30000000000000004).
pub fn number(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use autolevel::level::Options;
    use autolevel::recipe::{option, set_option, OPTIONS};

    #[test]
    fn every_option_has_a_control_and_a_line_of_help() {
        for name in OPTIONS {
            assert!(!control(name).1.is_empty(), "{name} has no help");
        }
    }

    #[test]
    fn every_word_offered_is_taken_and_every_default_lies_in_its_range() {
        let base = Options::default();
        for name in OPTIONS {
            let now = option(&base, name).unwrap();
            match control(name).0 {
                Control::Choice(words) => {
                    assert!(words.contains(&now.as_str()), "{name}: default {now} not offered");
                    for w in words {
                        let mut o = base.clone();
                        set_option(&mut o, name, w).unwrap_or_else(|e| panic!("{name} = {w}: {e}"));
                        assert_eq!(option(&o, name).unwrap(), *w);
                    }
                }
                Control::Switch => assert!(now == "true" || now == "false", "{name} is {now}"),
                Control::Whole { lo, hi, .. } => {
                    let v: i64 = now.parse().unwrap();
                    assert!((lo..=hi).contains(&v), "{name}: default {v} outside {lo}..{hi}");
                    for v in [lo, hi] {
                        set_option(&mut base.clone(), name, &v.to_string()).unwrap_or_else(|e| panic!("{name} = {v}: {e}"));
                    }
                }
                Control::Share { lo, hi, .. } => {
                    let v: f64 = now.parse().unwrap();
                    assert!(lo <= v && v <= hi, "{name}: default {v} outside {lo}..{hi}");
                    for v in [lo, hi] {
                        set_option(&mut base.clone(), name, &number(v)).unwrap_or_else(|e| panic!("{name} = {v}: {e}"));
                    }
                }
                Control::Site => {
                    assert_eq!(now, "default");
                    let mut o = base.clone();
                    set_option(&mut o, name, &format!("{}x{}", SITE_WIDE.0, SITE_DEEP.1)).unwrap();
                    assert_eq!(option(&o, name).unwrap(), "24x160");
                }
            }
        }
    }

    #[test]
    fn a_slider_step_comes_back_as_the_number_and_the_default_as_the_default() {
        assert_eq!(number(0.05 * 6.0), "0.3");
        assert_eq!(number(10.0), "10");
        assert_eq!(number(0.5), "0.5");
        let mut o = Options::default();
        set_option(&mut o, "clutter", &number(0.05 * 10.0)).unwrap();
        assert_eq!(o, Options::default(), "a slider put back on the default leaves the recipe as it was");
    }

    #[test]
    fn the_scales_offered_are_the_generators() {
        use autolevel::place::Scale;
        assert_eq!(SCALES, [Scale::Grand.name(), Scale::Standard.name(), Scale::Towering.name()]);
    }

    #[test]
    fn felicity_reads_only_the_washroom() {
        assert!(read_by("washroom", "felicity") && !read_by("washroom", "complex"));
        assert!(read_by("size", "complex") && !read_by("size", "felicity"));
        assert!(read_by("style", "felicity") && read_by("style", "complex"));
    }
}
