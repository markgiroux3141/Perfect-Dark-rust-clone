//! The port against its answer key: every stage must reproduce what the Python wrote for the
//! same seeds and options (`tests/key/*.json`, from `python scripts/autolevel_key.py`), or, where
//! a rule has been changed on purpose since, what the Rust gave when it was (`keyfile`: the key
//! re-written with `AUTOLEVEL_KEY_REWRITE=1`).

use autolevel::build::{Build, Fix};
use autolevel::deadends::{self, DeadEnd};
use autolevel::furnish::{self, Interior, RampIn};
use autolevel::level::{self, Level, Options, Step};
use autolevel::network::Network;
use autolevel::place::{self, Scale, Siting};
use autolevel::plan::{Box3, Point3, Room};
use autolevel::pyrand::PyRandom;
use autolevel::route::{Piece, Point, Route};
use autolevel::scatter::Lot;
use autolevel::sha512::sha512;
use autolevel::space::Door;
use autolevel::{cells, spiral};
use keyfile::Tree;
use serde_json::Value;

mod keyfile;

/// Where a stage's key is. (`AUTOLEVEL_KEY_DIR` names another folder of keys, written the same
/// way: for holding one stage to more levels than the key has, with the test's name as a filter.)
fn key_path(name: &str) -> String {
    let dir = std::env::var("AUTOLEVEL_KEY_DIR").unwrap_or_else(|_| format!("{}/tests/key", env!("CARGO_MANIFEST_DIR")));
    format!("{dir}/{name}.json")
}

fn key(name: &str) -> &'static Value {
    keyfile::read(&key_path(name), "run python scripts/autolevel_key.py")
}

fn ints(v: &Value) -> Vec<i64> {
    v.as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect()
}

/// Floats are in the key as their 64 bits, so no JSON reader's rounding comes into it.
fn bits(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect()
}

#[test]
fn sha512_is_pythons() {
    for case in key("rng")["sha512"].as_array().unwrap() {
        let text = case[0].as_str().unwrap();
        let hex: String = sha512(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, case[1].as_str().unwrap(), "sha512 of {text:?}");
    }
}

#[test]
fn every_random_call_draws_what_pythons_does() {
    let streams = key("rng");
    let mut calls = 0;
    for stream in streams["streams"].as_array().unwrap() {
        let seed = &stream["seed"];
        let mut r = match seed.as_str() {
            Some(text) => PyRandom::named(text),
            None => PyRandom::from_int(seed.as_u64().unwrap()),
        };
        for call in stream["calls"].as_array().unwrap() {
            let what = call[0].as_str().unwrap();
            let at = format!("seed {seed}, {call}");
            let last = call.as_array().unwrap().last().unwrap();
            match what {
                "random" => assert_eq!(bits(last).iter().map(|_| r.random().to_bits()).collect::<Vec<_>>(), bits(last), "{at}"),
                "bits32" => assert_eq!(ints(last).iter().map(|_| r.getrandbits(32) as i64).collect::<Vec<_>>(), ints(last), "{at}"),
                "below" => {
                    let n = call[1].as_u64().unwrap();
                    assert_eq!(ints(last).iter().map(|_| r.below(n) as i64).collect::<Vec<_>>(), ints(last), "{at}");
                }
                "randint" => {
                    let (a, b) = (call[1].as_i64().unwrap(), call[2].as_i64().unwrap());
                    assert_eq!(ints(last).iter().map(|_| r.randint(a, b)).collect::<Vec<_>>(), ints(last), "{at}");
                }
                "randrange" => {
                    let (a, b, step) = (call[1].as_i64().unwrap(), call[2].as_i64().unwrap(), call[3].as_i64().unwrap());
                    assert_eq!(ints(last).iter().map(|_| r.randrange(a, b, step)).collect::<Vec<_>>(), ints(last), "{at}");
                }
                "choice" => {
                    let seq: Vec<i64> = (0..call[1].as_i64().unwrap()).collect();
                    assert_eq!(ints(last).iter().map(|_| *r.choice(&seq)).collect::<Vec<_>>(), ints(last), "{at}");
                }
                "shuffle" => {
                    let mut xs: Vec<i64> = (0..call[1].as_i64().unwrap()).collect();
                    r.shuffle(&mut xs);
                    assert_eq!(xs, ints(last), "{at}");
                }
                "uniform" => {
                    let (a, b) = (f64::from_bits(call[1].as_u64().unwrap()), f64::from_bits(call[2].as_u64().unwrap()));
                    assert_eq!(bits(last).iter().map(|_| r.uniform(a, b).to_bits()).collect::<Vec<_>>(), bits(last), "{at}");
                }
                other => panic!("no such call in the key: {other}"),
            }
            calls += 1;
        }
    }
    assert!(calls > 300, "the key has {calls} calls");
}

/// A lot as the key writes it: [key, kind, x0, y0, x1, y1, floor, top, host].
fn row(a: &Lot) -> Value {
    serde_json::json!([a.key, a.kind, a.x0, a.y0, a.x1, a.y1, a.floor, a.top, a.host])
}

fn rows(lots: &[Lot]) -> Value {
    Value::Array(lots.iter().map(row).collect())
}

fn siting(options: &Value) -> Siting {
    let mut o = Siting::default();
    for (name, v) in options.as_object().unwrap() {
        match name.as_str() {
            "scale" => o.scale = if v == "standard" { Scale::Standard } else { Scale::Grand },
            "size" => o.size = v.as_f64().unwrap(),
            "site" => o.site = Some((v[0].as_i64().unwrap() as i32, v[1].as_i64().unwrap() as i32)),
            "shift" => o.shift = v.as_i64().unwrap() as i32,
            // (the level's other options: see options())
            "walkway" | "risers" | "vents" | "second" | "loops" | "level_change" => {}
            "cover" | "clutter" | "crates" | "shapes" | "drops" | "grates" | "windows" => {}
            "dead_ends" | "dead_corridor" | "dead_walk" | "dead_climb" => {}
            other => panic!("the key has an option the port does not know: {other}"),
        }
    }
    o
}

fn options(options: &Value) -> Options {
    // (the Python's levels have no flooded or shaped rooms, which the Rust lays by default:
    // `level::FLOODED`, `level::SHAPED`)
    let mut o = Options { siting: siting(options), flooded: 0.0, shaped: 0.0, ..Options::default() };
    if let Some(v) = options.get("walkway") {
        o.walkway = v.as_i64().unwrap() as i32;
    }
    if let Some(v) = options.get("risers") {
        o.risers = v.as_u64().unwrap() as usize;
    }
    if let Some(v) = options.get("vents") {
        o.vents = v.as_u64().unwrap() as usize;
    }
    if let Some(v) = options.get("second") {
        o.second = v.as_i64().unwrap() as i32;
    }
    if let Some(v) = options.get("loops") {
        o.loops = v.as_f64().unwrap();
    }
    if let Some(v) = options.get("level_change") {
        o.level_change = v.as_f64().unwrap();
    }
    if let Some(v) = options.get("cover") {
        o.cover = level::COVER.into_iter().find(|c| v == c).unwrap_or_else(|| panic!("the key has a cover the port does not know: {v}"));
    }
    if let Some(v) = options.get("clutter") {
        o.clutter = v.as_f64().unwrap();
    }
    if let Some(v) = options.get("crates") {
        o.crates = v.as_bool().unwrap();
    }
    if let Some(v) = options.get("shapes") {
        o.shapes = v.as_bool().unwrap();
    }
    if let Some(v) = options.get("drops") {
        o.drops = v.as_u64().unwrap() as usize;
    }
    if let Some(v) = options.get("grates") {
        o.grates = v.as_u64().unwrap() as usize;
    }
    if let Some(v) = options.get("windows") {
        o.windows = v.as_u64().unwrap() as usize;
    }
    if let Some(v) = options.get("dead_ends") {
        o.dead_ends = v.as_bool().unwrap();
    }
    if let Some(v) = options.get("dead_corridor") {
        o.dead_corridor = v.as_i64().unwrap() as i32;
    }
    if let Some(v) = options.get("dead_walk") {
        o.dead_walk = v.as_i64().unwrap() as i32;
    }
    if let Some(v) = options.get("dead_climb") {
        o.dead_climb = v.as_i64().unwrap() as i32;
    }
    o
}

fn rect_json(q: cells::Rect) -> Value {
    serde_json::json!([q.0, q.1, q.2, q.3])
}

fn ramp_json(p: &spiral::Ramp) -> Value {
    serde_json::json!([p.lo, p.hi, rect_json(p.rect), p.dir, rect_json(p.top), rect_json(p.foot)])
}

#[test]
fn rooms_are_what_the_router_is_given_in_python() {
    let (mut rooms, mut pods, mut spires) = (0, 0, 0);
    for case in key("spaces").as_array().unwrap() {
        let seed = case["seed"].as_u64().unwrap();
        let at = format!("seed {seed}, options {}", case["options"]);
        let got = level::stage_spaces(seed, &options(&case["options"])).unwrap_or_else(|e| panic!("{at}: {e}"));
        let want = case["spaces"].as_array().unwrap();
        assert_eq!(
            got.sp.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(),
            want.iter().map(|w| w["key"].as_str().unwrap()).collect::<Vec<_>>(),
            "{at}: rooms"
        );
        for (s, w) in got.sp.iter().zip(want) {
            let at = format!("{at}, {}", s.key);
            let decks = match &s.decks {
                None => Value::Null,
                Some(d) => Value::Object(d.iter().map(|(z, c)| (z.to_string(), serde_json::json!(cells::runs(c)))).collect()),
            };
            let pool = s.pool.map_or(serde_json::json!([]), |p| serde_json::json!([p.0, p.1, p.2, p.3, p.4]));
            let mine = serde_json::json!({
                "key": s.key, "kind": s.kind, "rect": rect_json(s.rect()), "floor": s.floor, "top": s.top, "host": s.host,
                "pool": pool, "storeys": s.storeys(), "door_levels": s.door_levels(), "vent_levels": s.vent_levels(),
                "decks": decks, "clear": cells::runs(&s.clear), "preset": s.preset.iter().map(ramp_json).collect::<Vec<_>>(),
            });
            for (name, v) in w.as_object().unwrap() {
                assert_eq!(&mine[name], v, "{at}: {name}");
            }
            rooms += 1;
            spires += s.decks.is_some() as usize;
        }
        let spin: serde_json::Map<String, Value> = got.spin.iter().map(|(k, (start, turn))| (k.clone(), serde_json::json!([start, turn]))).collect();
        assert_eq!(Value::Object(spin), case["spin"], "{at}: spin");
        assert_eq!(serde_json::json!(got.dropped), case["dropped"], "{at}: pods left out");
        let access: serde_json::Map<String, Value> = got
            .access
            .iter()
            .map(|(k, a)| {
                let v = serde_json::json!({"side": a.side.to_string(), "door": rect_json(a.door), "deck": cells::runs(&a.deck),
                                           "pad": cells::runs(&a.pad), "length": a.length, "island": a.island});
                (k.clone(), v)
            })
            .collect();
        pods += access.len();
        assert_eq!(Value::Object(access), case["access"], "{at}: pods' ways in");
    }
    assert!(rooms > 400 && pods > 20 && spires > 20, "{rooms} rooms, {pods} pods, {spires} spirals in the key");
}

#[test]
fn rooms_fall_settle_and_draw_together_as_in_python() {
    let cases = key("lots");
    let (mut made, mut failed) = (0, 0);
    for case in cases.as_array().unwrap() {
        let seed = case["seed"].as_u64().unwrap();
        let o = siting(&case["options"]);
        let at = format!("seed {seed}, options {}", case["options"]);
        let (kinds, site) = place::kinds_and_site(&o);
        assert_eq!(serde_json::json!([site.0, site.1]), case["site"], "{at}: site");
        assert_eq!(Value::Array(kinds.iter().map(|k| serde_json::json!([k.name, k.count])).collect()), case["kinds"], "{at}: kinds");
        match place::lots(seed, &o) {
            Err(why) => {
                assert_eq!(Value::String(why), case["failed"], "{at}");
                failed += 1;
            }
            Ok(lots) => {
                assert!(case.get("failed").is_none(), "{at}: Python found no room: {}", case["failed"]);
                assert_eq!(rows(&lots.scattered), case["scattered"], "{at}: scattered");
                assert_eq!(rows(&lots.settled), case["settled"], "{at}: settled");
                assert_eq!(rows(&lots.placed), case["placed"], "{at}: placed");
                made += 1;
            }
        }
    }
    assert!(made >= 90 && failed >= 1, "{made} cases made lots and {failed} failed: the key should have both");
}

fn rect(v: &Value) -> cells::Rect {
    let r = ints(v);
    (r[0] as i32, r[1] as i32, r[2] as i32, r[3] as i32)
}

#[test]
fn spirals_are_laid_out_as_in_python() {
    let (mut laid, mut none) = (0, 0);
    for case in key("spiral").as_array().unwrap() {
        let storeys: Vec<i32> = ints(&case["storeys"]).iter().map(|&z| z as i32).collect();
        let (start, turn) = (case["start"].as_u64().unwrap() as usize, case["turn"].as_i64().unwrap() as i32);
        let at = format!("rect {}, {} storeys, start {start}, turn {turn}", case["rect"], storeys.len());
        match spiral::layout(rect(&case["rect"]), &storeys, start, turn, 2) {
            None => {
                assert!(case.get("ramps").is_none(), "{at}: Python laid one out");
                none += 1;
            }
            Some(lay) => {
                let ramps: Vec<Value> = lay.ramps.iter().map(ramp_json).collect();
                assert_eq!(Value::Array(ramps), case["ramps"], "{at}: ramps");
                let decks: serde_json::Map<String, Value> = lay.decks.iter().map(|(z, cs)| (z.to_string(), serde_json::json!(cells::runs(cs)))).collect();
                assert_eq!(Value::Object(decks), case["decks"], "{at}: decks");
                assert_eq!(serde_json::json!(cells::runs(&lay.clear)), case["clear"], "{at}: clear");
                laid += 1;
            }
        }
    }
    assert!(laid >= 100 && none >= 40, "{laid} spirals laid out and {none} too small: the key should have both");
}

/// Every number as a float, so 67 and 67.0 are the same (the key's costs and centre lines are
/// whole or half numbers, which a float holds exactly).
fn floats(v: &Value) -> Value {
    match v {
        Value::Number(n) => serde_json::json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(floats).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), floats(x))).collect()),
        other => other.clone(),
    }
}

/// A door as the key writes it: [room, side, a, level, width, to].
fn door_json(d: &Door) -> Value {
    serde_json::json!([d.room, d.side.to_string(), d.a, d.level, d.width, d.to])
}

fn point_json(p: Point) -> Value {
    serde_json::json!([p.0, p.1, p.2])
}

fn piece_json(pc: &Piece) -> Value {
    let cells: serde_json::Map<String, Value> = pc.cells.iter().map(|(z, c)| (z.to_string(), serde_json::json!(cells::runs(c)))).collect();
    assert_eq!(cells.len(), pc.cells.len(), "{}: a storey twice", pc.key);
    serde_json::json!({
        "key": pc.key, "kind": pc.kind, "floor": pc.floor, "cells": cells,
        "ramp": pc.ramp.map_or(vec![], |(foot, top)| vec![point_json(foot), point_json(top)]),
        "ramp_cells": pc.ramp_cells.map_or(serde_json::json!([]), rect_json),
        "ducts": pc.ducts.iter().map(|((lo, hi), rect)| serde_json::json!([[point_json(*lo), point_json(*hi)], rect_json(*rect)])).collect::<Vec<_>>(),
    })
}

fn route_json(r: &Route) -> Value {
    serde_json::json!({
        "a": door_json(&r.a), "b": door_json(&r.b), "cost": r.cost,
        "path": r.path.iter().map(|p| point_json(*p)).collect::<Vec<_>>(),
        "pieces": r.pieces.iter().map(piece_json).collect::<Vec<_>>(),
    })
}

/// Every route against the key's rows, in the order they were made.
fn assert_routes(at: &str, routes: &[Route], want: &Value) {
    let want = want.as_array().unwrap();
    for (n, (r, w)) in routes.iter().zip(want).enumerate() {
        let at = format!("{at}, route {n} ({} to {})", w["a"][0], w["b"][0]);
        let mine = floats(&route_json(r));
        let w = floats(w);
        for name in ["a", "b", "cost", "path"] {
            assert_eq!(mine[name], w[name], "{at}: {name}");
        }
        let pieces = w["pieces"].as_array().unwrap();
        for (pc, wp) in mine["pieces"].as_array().unwrap().iter().zip(pieces) {
            for (name, v) in wp.as_object().unwrap() {
                assert_eq!(&pc[name], v, "{at}, piece {}: {name}", wp["key"]);
            }
        }
        assert_eq!(r.pieces.len(), pieces.len(), "{at}: pieces");
        assert_eq!(mine, w, "{at}");
    }
    assert_eq!(routes.len(), want.len(), "{at}: routes");
}

/// Every room's doors against the key's, the rooms in the order they were scattered.
fn assert_doors(at: &str, net: &Network, want: &Value) {
    let doors: Vec<(&str, Value)> = net.spaces().iter().map(|s| (s.key.as_str(), Value::Array(net.doors(&s.key).iter().map(door_json).collect()))).collect();
    let want = want.as_object().unwrap();
    assert_eq!(doors.len(), want.len(), "{at}: rooms");
    for (room, mine) in doors {
        assert_eq!(Some(&mine), want.get(room), "{at}: {room}'s doors");
    }
}

#[test]
fn corridors_stubs_and_vents_are_routed_as_in_python() {
    let (mut cases, mut routes, mut ramps, mut vents, mut stubs, mut unrouted) = (0, 0, 0, 0, 0, 0);
    for case in key("routes").as_array().unwrap() {
        let seed = case["seed"].as_u64().unwrap();
        let at = format!("seed {seed}, options {}", case["options"]);
        let o = options(&case["options"]);
        let rooms = level::stage_spaces(seed, &o).unwrap_or_else(|e| panic!("{at}: {e}"));
        let net = match level::stage_network(seed, &o, &rooms.sp) {
            Err(why) => {
                assert_eq!(Value::String(why), case["failed"], "{at}");
                continue;
            }
            Ok(net) => net,
        };
        assert!(case.get("failed").is_none(), "{at}: Python made no network: {}", case["failed"]);
        assert_routes(&at, net.routes(), &case["routes"]);
        routes += net.routes().len();
        ramps += net.routes().iter().map(|r| r.ramps()).sum::<usize>();
        vents += net.routes().iter().filter(|r| r.pieces[0].kind == "vent").count();
        stubs += net.routes().iter().filter(|r| r.pieces[0].kind == "stub").count();
        let pairs = |list: &[(String, String)]| serde_json::json!(list.iter().map(|(a, b)| [a, b]).collect::<Vec<_>>());
        assert_eq!(pairs(&net.pods), case["pods"], "{at}: pods");
        assert_eq!(pairs(&net.failed), case["unrouted"], "{at}: links that found no corridor");
        unrouted += net.failed.len();
        assert_doors(&at, &net, &case["doors"]);
        cases += 1;
    }
    assert!(
        cases >= 16 && routes > 600 && ramps > 100 && vents > 40 && stubs > 150 && unrouted >= 3,
        "{cases} levels, {routes} routes, {ramps} ramps, {vents} vents, {stubs} stubs, {unrouted} unrouted in the key"
    );
}

fn point3(p: Point3) -> Value {
    serde_json::json!([p.0, p.1, p.2])
}

fn box_json(b: &Box3) -> Value {
    serde_json::json!([point3(b.0), point3(b.1)])
}

fn rail_json(r: &furnish::Rail) -> Value {
    serde_json::json!([point3(r.0), point3(r.1), point3(r.2)])
}

fn ramp_in_json(r: &RampIn) -> Value {
    serde_json::json!([r.lo, r.hi, rect_json(r.rect), r.d, cells::runs(&r.top), cells::runs(&r.foot)])
}

fn storeys_json(by: &std::collections::BTreeMap<i32, cells::Cells>) -> Value {
    Value::Object(by.iter().map(|(z, c)| (z.to_string(), serde_json::json!(cells::runs(c)))).collect())
}

/// A room's inside as the key writes it, the fields in the order the furnisher makes them (so
/// the first one that differs points at one function).
fn interior_json(it: &Interior) -> Vec<(&'static str, Value)> {
    use serde_json::json;
    vec![
        ("levels", json!(it.levels)),
        ("tower", json!(it.tower)),
        ("balcony", json!(it.balcony)),
        ("layout", json!(it.layout)),
        ("cover_mode", json!(it.cover_mode)),
        ("pillar", json!(it.pillar)),
        ("shell", storeys_json(&it.shell)),
        ("roof", storeys_json(&it.roof)),
        ("piers", json!(it.piers.iter().map(|p| json!([p.0, p.1, p.2, p.3, p.4, p.5])).collect::<Vec<_>>())),
        ("pool", json!(cells::runs(&it.pool))),
        ("bridge", json!(cells::runs(&it.bridge))),
        ("extra_solids", json!(it.extra_solids.iter().map(|c| json!([c.0, c.1, c.2, c.3, c.4, c.5, c.6])).collect::<Vec<_>>())),
        ("hole", json!(cells::runs(&it.hole))),
        ("bridges", json!(it.bridges)),
        ("claim", json!(it.claim)),
        ("ramps", json!(it.ramps.iter().map(ramp_in_json).collect::<Vec<_>>())),
        ("deck", storeys_json(&it.deck)),
        ("shaped", json!(it.shaped)),
        ("cover", json!(it.cover.iter().map(|c| json!([c.0, c.1, c.2, c.3, c.4, c.5])).collect::<Vec<_>>())),
        ("hole_open", json!(it.hole_open)),
        ("rails", json!(it.rails.iter().map(rail_json).collect::<Vec<_>>())),
        ("ramp_rails", json!(it.ramp_rails().iter().map(rail_json).collect::<Vec<_>>())),
        ("boxes", json!(it.boxes().iter().map(box_json).collect::<Vec<_>>())),
        ("solids", json!(it.solids().iter().map(|(lo, hi, tag)| json!([point3(*lo), point3(*hi), tag])).collect::<Vec<_>>())),
        ("plan_ramps", json!(it.plan_ramps().iter().map(|r| json!([point3(r.start), point3(r.end), r.width, r.kind, r.open_sides])).collect::<Vec<_>>())),
        ("door_nodes", json!(it.door_nodes().iter().map(|n| n.map(|n| json!([n.0, n.1, n.2]))).collect::<Vec<_>>())),
    ]
}

/// A room's inside against the key's, field by field.
fn assert_interior(at: &str, it: &Interior, want: &Value) {
    let w = want.as_object().unwrap();
    let mine = interior_json(it);
    for (name, v) in &mine {
        keyfile::agree(v, w.get(*name), |a, b| a == b, || format!("{at}: {name}"));
    }
    assert_eq!(mine.len(), w.len(), "{at}: fields");
}

/// The build's drops (each with its open side) and grates against the key's.
fn assert_openings(at: &str, b: &Build, case: &Value) {
    let sided: Vec<Value> = b.sided().iter().map(|(u, low, r, side)| serde_json::json!([u, low, rect_json(*r), side])).collect();
    assert_eq!(Value::Array(sided), case["drops"], "{at}: drops");
    let grated: Vec<Value> = b.grates.iter().map(|(piece, low, r, z)| serde_json::json!([piece, low, rect_json(*r), z])).collect();
    assert_eq!(Value::Array(grated), case["grates"], "{at}: grates");
}

#[test]
fn rooms_are_furnished_as_in_python() {
    let (mut cases, mut rooms, mut pods, mut ramps, mut cover, mut rails, mut drops, mut grates) = (0, 0, 0, 0, 0, 0, 0, 0);
    let (mut pools, mut shaped, mut bridges) = (0, 0, 0);
    let cases_key = key("interiors");
    for case in cases_key.as_array().unwrap() {
        let seed = case["seed"].as_u64().unwrap();
        let at = format!("seed {seed}, options {}", case["options"]);
        let o = options(&case["options"]);
        let spaces = level::stage_spaces(seed, &o).unwrap_or_else(|e| panic!("{at}: {e}"));
        let built =
            level::stage_network(seed, &o, &spaces.sp).and_then(|net| level::stage_interiors(seed, &o, net, spaces.access.clone(), spaces.spin.clone()));
        let b = match built {
            Err(why) => {
                assert_eq!(Value::String(why), case["failed"], "{at}");
                continue;
            }
            Ok(b) => b,
        };
        assert!(case.get("failed").is_none(), "{at}: Python made no interiors: {}", case["failed"]);
        assert_openings(&at, &b, case);
        drops += b.drops.len();
        grates += b.grates.len();
        // every room that is not a pod, in the order they were scattered
        let want = case["interiors"].as_object().unwrap();
        let hosts: Vec<&str> = b.spaces().iter().filter(|s| s.host.is_empty()).map(|s| s.key.as_str()).collect();
        assert_eq!(b.interiors.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), hosts, "{at}: rooms furnished");
        assert_eq!(b.interiors.len(), want.len(), "{at}: rooms furnished");
        for (room, it) in &b.interiors {
            let at = format!("{at}, {room}");
            assert_interior(&at, it, want.get(room).unwrap_or_else(|| panic!("{at}: not in the key")));
            rooms += 1;
            ramps += it.ramps.len();
            cover += it.cover.len();
            rails += it.rails.len();
            pools += !it.pool.is_empty() as usize;
            shaped += !it.shaped.is_empty() as usize;
            bridges += it.bridges;
        }
        // each pod's own inside, as level.generate makes it
        let want = case["pods"].as_object().unwrap();
        let sp = b.spaces();
        let mut made = 0;
        for q in sp.iter().filter(|q| !q.host.is_empty()) {
            let at = format!("{at}, {}", q.key);
            let host = sp.iter().find(|s| s.key == q.host).unwrap();
            let others: Vec<&autolevel::space::Space> = sp.iter().filter(|x| x.host == q.host).collect();
            let acc = b.access.iter().find(|(k, _)| *k == q.key).map(|(_, a)| a);
            let pp = furnish::pod_interior(host, q, acc, &others);
            let w = floats(want.get(&q.key).unwrap_or_else(|| panic!("{at}: not in the key")));
            let boxes: Vec<Value> = pp.boxes.iter().map(box_json).collect();
            assert_eq!(floats(&Value::Array(boxes)), w["boxes"], "{at}: boxes");
            let glass: Vec<Value> = pp.glass.iter().map(|(a, b, z, f)| serde_json::json!([[a.0, a.1], [b.0, b.1], z, point3(*f)])).collect();
            assert_eq!(floats(&Value::Array(glass)), w["glass"], "{at}: glass");
            let door = pp.door.as_ref().map_or(Value::Null, |d| serde_json::json!([d.room, d.side.to_string(), d.a, d.level, d.width, d.to]));
            assert_eq!(floats(&door), w["door"], "{at}: door");
            made += 1;
        }
        assert_eq!(made, want.len(), "{at}: pods");
        pods += made;
        cases += 1;
    }
    assert!(
        cases >= 20 && rooms > 500 && pods > 40 && ramps > 400 && cover > 300 && rails > 1000 && drops >= 15 && grates >= 15 && pools >= 10 && shaped >= 20 && bridges >= 5,
        "{cases} levels, {rooms} rooms, {pods} pods, {ramps} ramps, {cover} cover, {rails} rails, {drops} drops, {grates} grates, {pools} pools, {shaped} shaped, {bridges} bridges in the key"
    );
    keyfile::finish(cases_key);
}

/// A dead end as the key writes it, the fields in the order the finder works them out.
fn dead_end_json(e: &DeadEnd) -> Vec<(&'static str, Value)> {
    use serde_json::json;
    let cut: serde_json::Map<String, Value> = e.cut.iter().map(|(room, top)| (room.clone(), json!(top))).collect();
    assert_eq!(cut.len(), e.cut.len(), "{}: a room cut twice", e.root);
    vec![
        ("root", json!(e.root)),
        ("way", json!([e.way.a, e.way.b, e.way.kind])),
        ("loop", json!(e.looped)),
        ("rooms", json!(e.rooms)),
        ("places", json!(e.places)),
        ("tip", json!(e.tip)),
        ("length", json!(e.length)),
        ("corridor", json!(e.corridor)),
        ("storeys", json!(e.storeys)),
        ("bare", json!(e.bare)),
        ("climb", json!(e.climb)),
        ("cut", Value::Object(cut)),
        ("leaf", json!(e.leaf)),
        ("routes", json!(e.routes)),
        ("why", json!(e.why)),
        ("allowed", json!(e.allowed)),
    ]
}

/// The dead ends found against the key's, in order, field by field.
fn assert_dead_ends(at: &str, ends: &[DeadEnd], want: &Value) {
    let want = want.as_array().unwrap();
    for (n, (e, w)) in ends.iter().zip(want).enumerate() {
        let w = w.as_object().unwrap();
        let mine = dead_end_json(e);
        for (name, v) in &mine {
            keyfile::agree(v, w.get(*name), |a, b| floats(a) == floats(b), || format!("{at}, dead end {n} (off {}): {name}", e.root));
        }
        assert_eq!(mine.len(), w.len(), "{at}, dead end {n}: fields");
    }
    assert_eq!(ends.len(), want.len(), "{at}: dead ends");
}

/// What was done about a dead end, as the Python's dict.
fn fix_json(f: &Fix) -> Value {
    use serde_json::json;
    match f {
        Fix::Joined { room, storey, to, by, kind, length, ramps } => {
            json!({"joined": room, "storey": storey, "to": to, "by": by, "kind": kind, "length": length, "ramps": ramps})
        }
        Fix::Cut { room, above } => json!({"cut": room, "above": above}),
        Fix::Dropped { room } => json!({"dropped": room}),
    }
}

#[test]
fn dead_ends_are_found_and_put_right_as_in_python() {
    use serde_json::json;
    let (mut cases, mut places, mut ways, mut found, mut bad, mut listed) = (0, 0, 0, 0, 0, 0);
    let (mut joined, mut cut, mut dropped, mut again, mut left) = (0, 0, 0, 0, 0);
    let cases_key = key("deadends");
    for case in cases_key.as_array().unwrap() {
        let seed = case["seed"].as_u64().unwrap();
        let at = format!("seed {seed}, options {}", case["options"]);
        let o = options(&case["options"]);
        let spaces = level::stage_spaces(seed, &o).unwrap_or_else(|e| panic!("{at}: {e}"));
        let net = level::stage_network(seed, &o, &spaces.sp).unwrap_or_else(|e| panic!("{at}: {e}"));
        let mut b = level::stage_interiors(seed, &o, net, spaces.access.clone(), spaces.spin.clone()).unwrap_or_else(|e| panic!("{at}: {e}"));
        let first: Vec<(String, Vec<(&'static str, Value)>)> = b.interiors.iter().map(|(k, it)| (k.clone(), interior_json(it))).collect();

        // the level as first furnished: its places, the ways between them, and what hangs off it
        let (g, ends) = b.ends();
        let want = case["places"].as_array().unwrap();
        for (n, (p, w)) in g.places().iter().zip(want).enumerate() {
            assert_eq!(&json!([p.key, p.room, p.level, p.kind]), w, "{at}: place {n}");
        }
        assert_eq!(g.places().len(), want.len(), "{at}: places");
        let want = case["ways"].as_array().unwrap();
        for (n, (w, row)) in g.ways.iter().zip(want).enumerate() {
            let mine = json!([w.a, w.b, w.kind, w.length, [w.pa.0, w.pa.1, w.pa.2], [w.pb.0, w.pb.1, w.pb.2], w.oneway, w.route]);
            assert_eq!(floats(&mine), floats(row), "{at}: way {n}");
        }
        assert_eq!(g.ways.len(), want.len(), "{at}: ways");
        assert_dead_ends(&format!("{at}, as first furnished"), &ends, &case["found"]);
        let want = case["joins"].as_array().unwrap();
        let long: Vec<&DeadEnd> = ends.iter().filter(|e| !e.allowed).collect();
        for (n, (e, rows)) in long.iter().zip(want).enumerate() {
            let rows = rows.as_array().unwrap();
            let mine = deadends::joins(&g, b.spaces(), e, deadends::REACH, deadends::STOREYS, deadends::EACH);
            for (m, ((room, storey, other, levels, sep), row)) in mine.iter().zip(rows).enumerate() {
                assert_eq!(floats(&json!([room, storey, other, levels, sep])), floats(row), "{at}, dead end {n} (off {}): join {m}", e.root);
            }
            assert_eq!(mine.len(), rows.len(), "{at}, dead end {n} (off {}): joins", e.root);
            listed += mine.len();
        }
        assert_eq!(long.len(), want.len(), "{at}: dead ends not allowed");
        places += g.places().len();
        ways += g.ways.len();
        found += ends.len();
        bad += long.len();

        // the pass (the key runs it whatever the options say)
        let done = if o.dead_ends {
            level::stage_dead_ends(b)
        } else {
            let run = b.resolve();
            run.map(|_| b)
        };
        let mut b = match done {
            Err(why) => {
                assert_eq!(Value::String(why), case["failed"], "{at}");
                continue;
            }
            Ok(b) => b,
        };
        assert!(case.get("failed").is_none(), "{at}: Python's pass failed: {}", case["failed"]);
        let want = case["fixes"].as_array().unwrap();
        for (n, (f, w)) in b.fixes.iter().zip(want).enumerate() {
            assert_eq!(floats(&fix_json(f)), floats(w), "{at}: fix {n}");
        }
        assert_eq!(b.fixes.len(), want.len(), "{at}: fixes");
        assert_routes(&at, b.net.routes(), &case["routes"]);
        assert_eq!(json!(b.spaces().iter().map(|s| &s.key).collect::<Vec<_>>()), case["rooms"], "{at}: rooms");
        assert_doors(&at, &b.net, &case["doors"]);
        let preset: serde_json::Map<String, Value> = b.spaces().iter().filter(|s| s.decks.is_some()).map(|s| (s.key.clone(), json!(s.preset.len()))).collect();
        assert_eq!(Value::Object(preset), case["preset"], "{at}: ramps left in each spiral");
        let caps: serde_json::Map<String, Value> = b.caps.iter().map(|(k, top)| (k.clone(), json!(top))).collect();
        assert_eq!(Value::Object(caps), case["caps"], "{at}: caps");
        assert_openings(&at, &b, case);
        // every room whose inside is not as it was first furnished, and no other
        let want = case["changed"].as_object().unwrap();
        let mut differ = 0;
        for (room, it) in &b.interiors {
            let was = first.iter().find(|(k, _)| k == room).map(|(_, fields)| fields);
            if was == Some(&interior_json(it)) {
                assert!(want.get(room).is_none(), "{at}, {room}: Python's pass changed its inside");
                continue;
            }
            let w = want.get(room).unwrap_or_else(|| panic!("{at}, {room}: its inside changed in the pass, and Python's did not"));
            assert_interior(&format!("{at}, {room} after the pass"), it, w);
            differ += 1;
        }
        assert_eq!(differ, want.len(), "{at}: rooms whose inside changed");
        let (_, ends) = b.ends();
        assert_dead_ends(&format!("{at}, after the pass"), &ends, &case["left"]);
        joined += b.fixes.iter().filter(|f| matches!(f, Fix::Joined { .. })).count();
        cut += b.fixes.iter().filter(|f| matches!(f, Fix::Cut { .. })).count();
        dropped += b.fixes.iter().filter(|f| matches!(f, Fix::Dropped { .. })).count();
        again += differ;
        left += ends.len();
        cases += 1;
    }
    assert!(
        cases >= 20 && places > 900 && ways > 1100 && found > 130 && bad >= 70 && listed > 300 && joined >= 70 && cut >= 9 && dropped >= 10 && again > 100 && left > 100,
        "{cases} levels, {places} places, {ways} ways, {found} dead ends ({bad} too long, {listed} joins listed), {joined} joined, {cut} cut, {dropped} dropped, {again} rooms furnished again, {left} dead ends left in the key"
    );
    keyfile::finish(cases_key);
}

/// The key's objects in the file's order, beside the key itself.
fn key_in_order(name: &str) -> Tree {
    let path = key_path(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    Tree::read(&text, &mut 0)
}

#[test]
fn the_keys_order_is_read_as_the_file_has_it() {
    let text = r#" {"b": [1, {"z": "a,}\"]", "y": -2.5e3}], "a": {}, "cA": null} "#;
    let tree = Tree::read(text, &mut 0);
    assert_eq!(tree.names(), ["b", "a", "cA"]);
    assert_eq!(tree.get("b").items()[1].names(), ["z", "y"]);
    assert!(tree.get("a").names().is_empty());
}

fn room_json(r: &Room) -> Vec<(&'static str, Value)> {
    use serde_json::json;
    vec![
        ("name", json!(r.name)),
        ("floor", json!(r.floor)),
        ("kind", json!(r.kind)),
        ("scheme", json!(r.scheme)),
        ("tint", json!(r.tint)),
        ("hub", json!(r.hub)),
        ("note", json!(r.note)),
        ("room_id", json!(r.room_id)),
        ("walkable", json!(r.walkable)),
        ("boxes", json!(r.boxes.iter().map(box_json).collect::<Vec<_>>())),
        ("solids", json!(r.solids.iter().map(|(lo, hi, tag)| json!([point3(*lo), point3(*hi), tag])).collect::<Vec<_>>())),
        ("ramps", json!(r.ramps.iter().map(|q| json!([point3(q.start), point3(q.end), q.width, q.kind, q.open_sides])).collect::<Vec<_>>())),
        ("rails", json!(r.rails.iter().map(rail_json).collect::<Vec<_>>())),
    ]
}

fn step_json(q: &Step) -> Value {
    match q.flags {
        None => serde_json::json!([q.x, q.y, q.z]),
        Some(flags) => serde_json::json!([q.x, q.y, q.z, flags]),
    }
}

/// A level's report as the Python's dict, its keys in the Python's order.
fn report_json(lv: &Level) -> Vec<(&'static str, Value)> {
    use serde_json::json;
    let r = &lv.report;
    let rows: Vec<Value> = r
        .dead_ends
        .iter()
        .map(|e| {
            json!({"root": e.root, "tip": e.tip, "rooms": e.rooms, "length": e.length, "corridor": e.corridor, "storeys": e.storeys,
                   "climb": e.climb, "allowed": e.allowed, "why": e.why})
        })
        .collect();
    vec![
        ("seed", json!(r.seed)),
        ("scale", json!(r.scale)),
        ("cover", json!(r.cover)),
        ("rooms", json!(r.rooms)),
        ("plan_rooms", json!(r.plan_rooms)),
        ("links", json!(r.links)),
        ("loops", json!(r.loops)),
        ("one_door_rooms", json!(r.one_door_rooms)),
        ("dead_ends", json!(rows)),
        ("dead_ends_long", json!(r.dead_ends_long)),
        ("camps", json!(r.camps)),
        ("dead_end_fixes", json!(r.dead_end_fixes.iter().map(fix_json).collect::<Vec<_>>())),
        ("hub_share", json!(r.hub_share)),
        ("diameter", json!(r.diameter)),
        ("size_ratio", json!(r.size_ratio)),
        ("largest_over_smallest", json!(r.largest_over_smallest)),
        ("tall_rooms", json!(r.tall_rooms)),
        ("tallest", json!(r.tallest)),
        ("largest", json!(r.largest)),
        ("high_rooms", json!(r.high_rooms)),
        ("spirals", json!(r.spirals)),
        ("spiral_storeys", json!(r.spiral_storeys)),
        ("switchbacks", json!(r.switchbacks)),
        ("bridges", json!(r.bridges)),
        ("layouts", json!(r.layouts)),
        ("shaped", json!(r.shaped)),
        ("pillars", json!(r.pillars)),
        ("crates", json!(r.crates)),
        ("centre_offset", json!(r.centre_offset)),
        ("floors", json!(r.floors)),
        ("extent", json!([r.extent.0, r.extent.1])),
        ("corridor_share", json!(r.corridor_share)),
        ("corridors", json!(r.corridors)),
        ("vents", json!(r.vents)),
        ("sloped_vents", json!(r.sloped_vents)),
        ("overlooks", json!(r.overlooks)),
        ("drops", json!(r.drops)),
        ("lightwells", json!(r.lightwells)),
        ("grates", json!(r.grates)),
        ("stubs", json!(r.stubs)),
        ("corridor_ramps", json!(r.corridor_ramps)),
        ("longest_corridor", json!(r.longest_corridor)),
        ("upper_doors", json!(r.upper_doors)),
        ("catwalk_rooms", json!(r.catwalk_rooms)),
        ("catwalk_cells", json!(r.catwalk_cells)),
        ("ring_rooms", json!(r.ring_rooms)),
        ("room_ramps", json!(r.room_ramps)),
        ("pods", json!(r.pods)),
        ("pods_dropped", json!(r.pods_dropped)),
        ("windows", json!(r.windows)),
        ("pools", json!(r.pools)),
        ("rails", json!(r.rails)),
        ("walks", json!(r.walks)),
        ("score", json!(r.score)),
    ]
}

/// A dict of the level's against the key's: the same names in the same order, and each value.
fn assert_dict(at: &str, what: &str, mine: &[(String, Value)], want: &Value, order: &Tree) {
    let names: Vec<&str> = mine.iter().map(|(k, _)| k.as_str()).collect();
    if keyfile::rewriting() && names != order.names() {
        // (re-writing the key, a dict whose names changed is taken whole, in its own order)
        let items: Vec<String> = mine.iter().map(|(k, v)| format!("{}: {}", serde_json::to_string(k).unwrap(), serde_json::to_string(v).unwrap())).collect();
        assert!(keyfile::take_text(want, format!("{{{}}}", items.join(", "))), "{at}: {what} is not in the key");
        return;
    }
    for (n, (name, w)) in names.iter().zip(order.names()).enumerate() {
        assert_eq!(*name, w, "{at}: {what} {n}");
    }
    assert_eq!(names.len(), order.names().len(), "{at}: {what}");
    for (name, v) in mine {
        keyfile::agree(v, want.get(name), |a, b| floats(a) == floats(b), || format!("{at}: {what} {name}"));
    }
}

/// Everything a finished level holds against the key's case, in the order the level is made
/// (so the first thing that differs points at one function): windows, the plan's rooms and
/// links, what the plan can't hold, the walks, the views and the report.
fn assert_level(at: &str, lv: &Level, case: &Value, order: &Tree) {
    use serde_json::json;
    let cell = |c: &cells::Cell| json!([c.0, c.1]);
    let pair = |p: (f64, f64)| json!([p.0, p.1]);
    let dict = |items: Vec<(String, Value)>, name: &str| assert_dict(at, name, &items, &case[name], order.get(name));
    let list = |items: Vec<Value>, name: &str| {
        // (re-writing the key, a list that differs is taken whole: its length may have changed)
        let all = Value::Array(items.clone());
        if keyfile::rewriting() && floats(&all) != floats(&case[name]) && keyfile::take(&case[name], &all) {
            return;
        }
        let want = case[name].as_array().unwrap();
        for (n, (mine, w)) in items.iter().zip(want).enumerate() {
            assert_eq!(floats(mine), floats(w), "{at}: {name} {n}");
        }
        assert_eq!(items.len(), want.len(), "{at}: {name}");
    };

    // what the dead-end pass left, as the level holds it
    list(lv.drops.iter().map(|(u, low, r, side)| json!([u, low, rect_json(*r), side])).collect(), "drops");
    list(lv.grates.iter().map(|(piece, low, r, z)| json!([piece, low, rect_json(*r), z])).collect(), "grates");
    list(lv.beams.iter().map(|(low, r, top, bottom)| json!([low, rect_json(*r), top, bottom])).collect(), "beams");
    list(lv.dropped.iter().map(|k| json!(k)).collect(), "dropped");
    list(lv.windows.iter().map(|w| json!([w.piece, w.hall, w.z, w.wall.iter().map(cell).collect::<Vec<_>>(), w.d, w.kind])).collect(), "windows");

    // the plan: the rooms in the order they were added (their names prove the random stream)
    let want = &case["rooms"];
    let rooms: Vec<(&str, &str)> = lv.plan.rooms.iter().map(|r| (r.key.as_str(), r.name.as_str())).collect();
    let wanted: Vec<(&str, &str)> = order.get("rooms").names().into_iter().map(|k| (k, want[k]["name"].as_str().unwrap())).collect();
    for (n, (mine, w)) in rooms.iter().zip(&wanted).enumerate() {
        assert_eq!(mine, w, "{at}: room {n}");
    }
    assert_eq!(rooms.len(), wanted.len(), "{at}: rooms");
    for r in &lv.plan.rooms {
        let w = want[&r.key].as_object().unwrap();
        let mine = room_json(r);
        for (name, v) in &mine {
            keyfile::agree(v, w.get(*name), |a, b| a == b, || format!("{at}, {}: {name}", r.key));
        }
        assert_eq!(mine.len(), w.len(), "{at}, {}: fields", r.key);
    }
    list(lv.plan.links.iter().map(|ln| json!([ln.a, ln.b, ln.kind])).collect(), "links");
    assert!(lv.plan.links.iter().all(|ln| ln.note.is_empty()), "{at}: a link with a note");
    assert_eq!(lv.plan.name, format!("Autolevel {}", lv.seed), "{at}: the plan's name");

    // what the plan can't hold
    list(lv.glass.iter().map(|(room, a, b, z, f)| json!([room, pair(*a), pair(*b), z, point3(*f)])).collect(), "glass");
    dict(lv.pools.iter().map(|(k, z)| (k.clone(), json!(z))).collect(), "pools");
    list(lv.ramp_rails.iter().map(|(room, a, b, f)| json!([room, point3(*a), point3(*b), point3(*f)])).collect(), "ramp_rails");
    dict(lv.moods.iter().map(|(k, v)| (k.clone(), json!(v))).collect(), "moods");
    dict(lv.ceilings.iter().map(|(k, v)| (k.clone(), json!(v))).collect(), "ceilings");
    list(lv.glows.iter().map(|(room, xy)| json!([room, [xy.0, xy.1]])).collect(), "glows");

    dict(lv.walks.iter().map(|(k, pts)| (k.clone(), json!(pts.iter().map(step_json).collect::<Vec<_>>()))).collect(), "walks");
    dict(lv.views.iter().map(|(k, v)| (k.clone(), json!([v.0, v.1, v.2, v.3, v.4]))).collect(), "views");
    let report: Vec<(String, Value)> = report_json(lv).into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    dict(report, "report");
}

#[test]
fn levels_are_finished_as_in_python() {
    let (mut cases, mut rooms, mut links, mut walks, mut views, mut windows, mut glass, mut glows) = (0, 0, 0, 0, 0, 0, 0, 0);
    let (mut fails, mut rails, mut vents, mut pools, mut climbs, mut drops) = (0, 0, 0, 0, 0, 0);
    let order = key_in_order("level");
    let cases_key = key("level");
    for (case, order) in cases_key.as_array().unwrap().iter().zip(order.items()) {
        let seed = case["seed"].as_u64().unwrap();
        let at = format!("seed {seed}, options {}", case["options"]);
        let lv = match level::generate(seed, &options(&case["options"])) {
            Err(why) => {
                assert_eq!(Value::String(why), case["failed"], "{at}");
                continue;
            }
            Ok(lv) => lv,
        };
        assert!(case.get("failed").is_none(), "{at}: Python made no level: {}", case["failed"]);
        assert_level(&at, &lv, case, order);
        // everything compared is everything the key has
        assert_eq!(order.names().len(), 18, "{at}: the key's fields");
        rooms += lv.plan.rooms.len();
        links += lv.plan.links.len();
        walks += lv.walks.len();
        views += lv.views.len();
        windows += lv.windows.len();
        glass += lv.glass.len();
        glows += lv.glows.len();
        fails += lv.walks.iter().filter(|(k, _)| k.starts_with('!')).count();
        rails += lv.windows.iter().filter(|w| w.kind == "rail").count();
        vents += lv.walks.iter().filter(|(k, _)| k.starts_with("vent_")).count();
        pools += lv.pools.len();
        climbs += lv.walks.iter().filter(|(k, _)| k.starts_with("climb_")).count();
        drops += lv.walks.iter().filter(|(k, _)| k.starts_with("drop_")).count();
        cases += 1;
    }
    assert!(
        cases >= 20 && rooms > 1200 && links > 1500 && walks > 1400 && views > 700 && windows >= 25 && glass > 150 && glows >= 20,
        "{cases} levels, {rooms} plan rooms, {links} links, {walks} walks, {views} views, {windows} windows, {glass} panes, {glows} glows in the key"
    );
    assert!(
        fails >= 20 && rails >= 2 && vents >= 40 && pools >= 10 && climbs >= 20 && drops >= 10,
        "{fails} walks that must fail, {rails} railed windows, {vents} vents, {pools} pools, {climbs} climbs, {drops} drops in the key"
    );
    keyfile::finish(cases_key);
}
