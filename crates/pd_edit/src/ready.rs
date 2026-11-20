//! "Ready to play?": what the Test step's checklist says about a level, worked
//! out from its layout and its simulants' graph. Each check is a line a
//! person can read, and a suggestion (never a refusal: nothing here stops a
//! level being played), with the fix the editor can make or show.

use crate::doc::{Doc, Item, Kind};

/// What a check's button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fix {
    /// Import the level (it has not been built yet).
    Build,
    /// Go to the Items step with this kind's add tool.
    Add(Kind),
    /// Put an ammo crate beside each of these weapons.
    CratesFor(Vec<usize>),
    /// Show the simulants' paths, the stranded places in red.
    ShowPaths,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub ok: bool,
    pub title: String,
    pub detail: String,
    /// The fix's button and what it does (for a check that isn't ok).
    pub fix: Option<(&'static str, Fix)>,
}

impl Check {
    fn ok(title: impl Into<String>, detail: impl Into<String>) -> Check {
        Check { ok: true, title: title.into(), detail: detail.into(), fix: None }
    }

    fn warn(title: impl Into<String>, detail: impl Into<String>, button: &'static str, fix: Fix) -> Check {
        Check { ok: false, title: title.into(), detail: detail.into(), fix: Some((button, fix)) }
    }
}

/// Spawn points a level wants: enough for eight players to start apart.
pub const SPAWNS_WANTED: usize = 8;

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The weapons no ammo crate belongs to.
pub fn weapons_without_ammo(doc: &Doc) -> Vec<usize> {
    let crates: Vec<usize> = (0..doc.count(Kind::Ammo)).filter_map(|index| doc.crate_weapon(Item { kind: Kind::Ammo, index })).collect();
    (0..doc.count(Kind::Weapon)).filter(|w| !crates.contains(w)).collect()
}

/// The checklist. `built`: the level has been imported and loaded;
/// `stranded`: the simulants' waypoints they can't reach and get back from
/// (`None` while the graph isn't loaded).
pub fn checks(doc: &Doc, built: bool, stranded: Option<usize>) -> Vec<Check> {
    let mut out = Vec::new();
    out.push(if built { Check::ok("The arena is built", "It is in the game under Arena, Custom.") } else { Check::warn("Not built yet", "The level has to be built before it can be played.", "Build it", Fix::Build) });
    let spawns = doc.count(Kind::Spawn);
    out.push(match spawns {
        0 => Check::warn("No spawn points", format!("Players need places to start; {SPAWNS_WANTED} or more is best."), "Add spawn points", Fix::Add(Kind::Spawn)),
        n if n < SPAWNS_WANTED => Check::warn(plural(n, "spawn point", "spawn points"), format!("Players will often start close together; {SPAWNS_WANTED} or more is best."), "Add more", Fix::Add(Kind::Spawn)),
        n => Check::ok(plural(n, "spawn point", "spawn points"), format!("Enough for {SPAWNS_WANTED} players to start apart.")),
    });
    let weapons = doc.count(Kind::Weapon);
    out.push(if weapons == 0 { Check::warn("No weapon spots", "Players will only have what they start with.", "Add weapons", Fix::Add(Kind::Weapon)) } else { Check::ok(plural(weapons, "weapon spot", "weapon spots"), "Each holds a gun from the weapon set chosen in the game.") });
    let dry = weapons_without_ammo(doc);
    if !dry.is_empty() {
        let which = dry.iter().map(|w| (w + 1).to_string()).collect::<Vec<_>>().join(", ");
        let title = if dry.len() == 1 { "1 weapon spot has no ammo".to_owned() } else { format!("{} weapon spots have no ammo", dry.len()) };
        out.push(Check::warn(title, format!("Spot {which} has no ammo crate, so its gun runs dry."), if dry.len() == 1 { "Add a crate" } else { "Add crates" }, Fix::CratesFor(dry)));
    } else if weapons > 0 {
        out.push(Check::ok("Every weapon spot has ammo", "Each has an ammo crate of its own."));
    }
    let hills = doc.count(Kind::Hill);
    out.push(if hills == 0 { Check::warn("No hills", "King of the Hill can't be played here.", "Add a hill", Fix::Add(Kind::Hill)) } else { Check::ok(plural(hills, "hill", "hills"), "King of the Hill can be played here.") });
    let mut teams: Vec<u8> = (0..doc.count(Kind::Base)).filter_map(|index| doc.team(Item { kind: Kind::Base, index })).collect();
    teams.sort_unstable();
    teams.dedup();
    out.push(match teams.len() {
        0 => Check::warn("No team bases", "Capture the Case needs a base for each team.", "Add a base", Fix::Add(Kind::Base)),
        1 => Check::warn("A base for only 1 team", "Capture the Case needs a base for each team.", "Add a base", Fix::Add(Kind::Base)),
        n => Check::ok(format!("Team bases for {n} teams"), "Capture the Case can be played here."),
    });
    match stranded {
        Some(0) => out.push(Check::ok("Simulants can go everywhere", "They can reach every part of the level and get back.")),
        Some(n) => out.push(Check::warn(format!("{} simulants can't get back from", plural(n, "place", "places")), "They can get there but not out, or not at all.", "Show me", Fix::ShowPaths)),
        None => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use pd_import::layout::Layout;

    fn level(spawns: usize, weapons: usize, crates: &[usize]) -> Doc {
        let mut doc = Doc::new(Layout::new(), true);
        for i in 0..spawns {
            doc.add(Kind::Spawn, Vec3::new(i as f32 * 300.0, 0.0, 0.0), 0.0, 0, None);
        }
        for i in 0..weapons {
            doc.add(Kind::Weapon, Vec3::new(i as f32 * 300.0, 0.0, 500.0), 0.0, 0, None);
        }
        for &w in crates {
            doc.add(Kind::Ammo, Vec3::new(w as f32 * 300.0 + 50.0, 0.0, 500.0), 0.0, 0, Some(w));
        }
        doc
    }

    #[test]
    fn a_bare_level_is_told_what_it_lacks() {
        let doc = level(0, 0, &[]);
        let c = checks(&doc, false, None);
        let fixes: Vec<_> = c.iter().filter_map(|c| c.fix.clone().map(|f| f.1)).collect();
        assert_eq!(fixes, vec![Fix::Build, Fix::Add(Kind::Spawn), Fix::Add(Kind::Weapon), Fix::Add(Kind::Hill), Fix::Add(Kind::Base)]);
    }

    #[test]
    fn weapons_without_crates_are_named() {
        let doc = level(10, 3, &[1]);
        assert_eq!(weapons_without_ammo(&doc), vec![0, 2]);
        let c = checks(&doc, true, Some(0));
        let dry = c.iter().find(|c| c.title.contains("no ammo")).unwrap();
        assert_eq!(dry.title, "2 weapon spots have no ammo");
        assert_eq!(dry.detail, "Spot 1, 3 has no ammo crate, so its gun runs dry.");
        assert_eq!(dry.fix, Some(("Add crates", Fix::CratesFor(vec![0, 2]))));
        assert!(c.iter().find(|c| c.title.contains("spawn")).unwrap().ok);
        assert!(c.last().unwrap().ok, "the graph");
    }

    #[test]
    fn few_spawns_and_stranded_places_are_suggestions() {
        let doc = level(3, 1, &[0]);
        let c = checks(&doc, true, Some(2));
        let s = c.iter().find(|c| c.title.contains("spawn")).unwrap();
        assert_eq!((s.ok, s.title.as_str()), (false, "3 spawn points"));
        assert_eq!(c.last().unwrap().title, "2 places simulants can't get back from");
    }
}
