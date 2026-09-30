//! The waypoint graph, cached per level (`custom/cache/nav/<code>.json`) and
//! keyed by the stage's geometry (`bg.json` and `tiles.json`): the generator
//! takes 10-60 s, and placing a level again after an edit in `pd_edit`
//! changes nothing it reads. A full import always generates it afresh (the
//! generator's own code may have changed); the cache serves `--place` and the
//! editor.

use std::path::Path;

use glam::Vec3;
use pd_sim::nav::{NavGraph, NavPad, NavWaygroup, NavWaypoint, PadFlags};
use pd_sim::stage::TileLevel;
use serde_json::{json, Value};

use crate::write::{pad_flags, write_json};
use crate::Paths;

const FORMAT: &str = "pd-nav-cache/1";

/// FNV-1a, 64 bits.
fn fnv1a(h: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(h, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// The key of the stage in `dir`: what the generator reads (the rooms and
/// the tiles).
pub fn stage_key(dir: &Path) -> Result<String, String> {
    let mut h = 0xcbf2_9ce4_8422_2325;
    for f in ["bg.json", "tiles.json"] {
        let b = std::fs::read(dir.join(f)).map_err(|e| format!("{}: {e}", dir.join(f).display()))?;
        h = fnv1a(h, &b);
    }
    Ok(format!("{h:016x}"))
}

fn to_json(key: &str, line: &str, g: &NavGraph) -> Value {
    json!({
        "format": FORMAT, "key": key, "report": line,
        "pads": g.pads.iter().map(|p| json!({"pos": [p.pos.x, p.pos.y, p.pos.z], "room": p.room, "flags": pad_flags(&p.flags)})).collect::<Vec<_>>(),
        "waypoints": g.waypoints.iter().map(|w| json!({"pad": w.padnum, "group": w.groupnum, "neighbours": w.neighbours})).collect::<Vec<_>>(),
        "waygroups": g.waygroups.iter().map(|w| json!({"neighbours": w.neighbours, "waypoints": w.waypoints})).collect::<Vec<_>>(),
    })
}

fn from_json(v: &Value) -> Option<NavGraph> {
    let ints = |x: &Value| -> Vec<i32> { x.as_array().into_iter().flatten().filter_map(|n| n.as_i64()).map(|n| n as i32).collect() };
    let pads = v["pads"]
        .as_array()?
        .iter()
        .map(|p| {
            let q = &p["pos"];
            Some(NavPad {
                pos: Vec3::new(q[0].as_f64()? as f32, q[1].as_f64()? as f32, q[2].as_f64()? as f32),
                room: p["room"].as_u64().map(|r| r as u16),
                flags: PadFlags::from_bits(p["flags"].as_u64()? as u32),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let waypoints = v["waypoints"].as_array()?.iter().map(|w| Some(NavWaypoint { padnum: w["pad"].as_u64()? as usize, groupnum: w["group"].as_u64()? as usize, neighbours: ints(&w["neighbours"]) })).collect::<Option<Vec<_>>>()?;
    let waygroups = v["waygroups"]
        .as_array()?
        .iter()
        .map(|w| NavWaygroup { neighbours: ints(&w["neighbours"]), waypoints: w["waypoints"].as_array().into_iter().flatten().filter_map(|n| n.as_u64()).map(|n| n as usize).collect() })
        .collect();
    if waypoints.iter().any(|w: &NavWaypoint| w.padnum >= pads.len()) {
        return None;
    }
    Some(NavGraph::new(pads, waypoints, waygroups))
}

/// The waypoint graph of the stage in `dir` (`level` loaded from it): the
/// cached one if its key matches and `fresh` isn't asked, else generated and
/// cached. Returns it and a report line.
pub fn graph(paths: &Paths, code: &str, dir: &Path, level: &TileLevel, fresh: bool) -> Result<(NavGraph, String), String> {
    let key = stage_key(dir)?;
    let path = paths.custom.join("cache").join("nav").join(format!("{code}.json"));
    if !fresh {
        let cached = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if let Some(v) = cached.filter(|v| v["format"] == FORMAT && v["key"] == key.as_str()) {
            if let Some(g) = from_json(&v) {
                return Ok((g, format!("{} (cached)", v["report"].as_str().unwrap_or("waypoints"))));
            }
        }
    }
    let (g, line) = crate::place::generate_graph(level);
    write_json(&path, &to_json(&key, &line, &g))?;
    Ok((g, line))
}
