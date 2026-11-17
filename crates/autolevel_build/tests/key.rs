//! The polygon stage against its answer key: for the key's seeds and options, `geometry::polys`
//! and `complex::plan_lights` must give exactly what the Python gave (`tests/key/polys.json` and
//! `lights.json`, from `python scripts/autolevel_key.py polys lights`).
//!
//! A polygon is a row `[zone, material, verts, normal, scale, band, slope, offset, uvs, beam]`
//! with every float as its 64 bits (and -0.0 as 0.0: see `fbits` in the key script). A room's
//! polygons are held to a SHA-512 of their rows' text, after `plan_polys` and at the end; the
//! first case has its rows in full, so a difference there is shown polygon by polygon. To see
//! another case's rows, set `AUTOLEVEL_POLYS_OUT` to a folder: each case's rows are written there
//! as `case<n>.json`, in the form `python scripts/autolevel_key.py --polys SEED out.json` writes.
//! (with two things more, which the key does not hold: `projected`, each polygon's UVs from
//! `csg::project_uvs`, and `intensity`, each beam's strength on the polygons it lights).

use autolevel::level::{self, Level, Options};
use autolevel::place::{Scale, Siting};
use autolevel::sha512::sha512;
use autolevel_build::complex as cx;
use autolevel_build::csg::{self, Poly};
use autolevel_build::{geometry, sunbeam};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

/// Where a stage's key is (`AUTOLEVEL_KEY_DIR` names another folder of keys written the same way).
fn key(name: &str) -> Value {
    let dir = std::env::var("AUTOLEVEL_KEY_DIR").unwrap_or_else(|_| format!("{}/tests/key", env!("CARGO_MANIFEST_DIR")));
    let path = format!("{dir}/{name}.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e} (run python scripts/autolevel_key.py polys lights)"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A case's `"options"` as the level's options (as the layout's own key test reads them).
fn options(options: &Value) -> Options {
    let mut o = Options::default();
    for (name, v) in options.as_object().unwrap() {
        let whole = || v.as_i64().unwrap();
        match name.as_str() {
            "scale" => o.siting.scale = if v == "standard" { Scale::Standard } else { Scale::Grand },
            "size" => o.siting.size = v.as_f64().unwrap(),
            "site" => o.siting.site = Some((v[0].as_i64().unwrap() as i32, v[1].as_i64().unwrap() as i32)),
            "shift" => o.siting.shift = whole() as i32,
            "walkway" => o.walkway = whole() as i32,
            "risers" => o.risers = whole() as usize,
            "vents" => o.vents = whole() as usize,
            "second" => o.second = whole() as i32,
            "loops" => o.loops = v.as_f64().unwrap(),
            "level_change" => o.level_change = v.as_f64().unwrap(),
            "cover" => o.cover = level::COVER.into_iter().find(|c| v == c).unwrap_or_else(|| panic!("the key has a cover the port does not know: {v}")),
            "clutter" => o.clutter = v.as_f64().unwrap(),
            "crates" => o.crates = v.as_bool().unwrap(),
            "shapes" => o.shapes = v.as_bool().unwrap(),
            "drops" => o.drops = whole() as usize,
            "grates" => o.grates = whole() as usize,
            "windows" => o.windows = whole() as usize,
            "dead_ends" => o.dead_ends = v.as_bool().unwrap(),
            "dead_corridor" => o.dead_corridor = whole() as i32,
            "dead_walk" => o.dead_walk = whole() as i32,
            "dead_climb" => o.dead_climb = whole() as i32,
            other => panic!("the key has an option the port does not know: {other}"),
        }
    }
    let _: &Siting = &o.siting;
    o
}

/// Run `f` over the items on as many threads as the machine has, results in the items' order.
fn par<T: Sync, R: Send>(items: &[T], f: impl Fn(usize, &T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(items.len().max(1));
    let next = AtomicUsize::new(0);
    let out: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() {
                    break;
                }
                *out[i].lock().unwrap() = Some(f(i, &items[i]));
            });
        }
    });
    out.into_iter().map(|m| m.into_inner().unwrap().expect("every item was run")).collect()
}

/// The key's levels, made once for both tests: (seed, options as the key has them, the level).
fn levels() -> &'static [(u64, Value, Level)] {
    static LEVELS: OnceLock<Vec<(u64, Value, Level)>> = OnceLock::new();
    LEVELS.get_or_init(|| {
        let cases = key("lights");
        let cases: Vec<(u64, Value)> = cases.as_array().unwrap().iter().map(|c| (c["seed"].as_u64().unwrap(), c["options"].clone())).collect();
        par(&cases, |_, (seed, o)| {
            let lv = level::generate(*seed, &options(o)).unwrap_or_else(|e| panic!("seed {seed} {o}: {e}"));
            (*seed, o.clone(), lv)
        })
    })
}

/// A float as the key writes it: its 64 bits, with -0.0 as 0.0.
fn fbits(x: f64) -> u64 {
    (x + 0.0).to_bits()
}

/// A polygon as the key's row.
fn poly_row(p: &Poly) -> Value {
    let s = &p.surface;
    let verts: Vec<i32> = p.verts.iter().flatten().copied().collect();
    let uvs: Option<Vec<u64>> = p.uvs.as_ref().map(|u| u.iter().flat_map(|t| [fbits(t.0), fbits(t.1)]).collect());
    json!([
        p.zone,
        s.material,
        verts,
        p.normal.map(fbits),
        [fbits(s.scale.0), fbits(s.scale.1)],
        s.band.map(fbits),
        fbits(s.slope),
        [fbits(s.offset.0), fbits(s.offset.1)],
        uvs,
        p.lit.map_or(-1, |b| b as i64)
    ])
}

/// A row with its floats as numbers, to read.
fn show(row: &Value) -> String {
    let f = |v: &Value| f64::from_bits(v.as_u64().unwrap());
    let fs = |v: &Value| v.as_array().unwrap().iter().map(f).collect::<Vec<_>>();
    let verts: Vec<Vec<i64>> = row[2].as_array().unwrap().chunks(3).map(|v| v.iter().map(|c| c.as_i64().unwrap()).collect()).collect();
    let band = row[5].as_u64().map(f64::from_bits);
    let uvs = row[8].as_array().map(|u| u.iter().map(f).collect::<Vec<_>>());
    format!(
        "{} {} verts {:?} normal {:?} scale {:?} band {:?} slope {} offset {:?} uvs {:?} lit {}",
        row[0],
        row[1],
        verts,
        fs(&row[3]),
        fs(&row[4]),
        band,
        f(&row[6]),
        fs(&row[7]),
        uvs,
        row[9]
    )
}

/// [[zone, polygons, SHA-512 of its rows' text joined by newlines]], zones in the order they first appear.
fn zone_digests(rows: &[Value]) -> Value {
    let mut by: Vec<(&str, Vec<String>)> = Vec::new();
    for row in rows {
        let zone = row[0].as_str().unwrap();
        let text = serde_json::to_string(row).unwrap();
        match by.iter_mut().find(|(z, _)| *z == zone) {
            Some((_, t)) => t.push(text),
            None => by.push((zone, vec![text])),
        }
    }
    let hex = |t: &[String]| sha512(t.join("\n").as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
    Value::Array(by.iter().map(|(z, t)| json!([z, t.len(), hex(t)])).collect())
}

/// Where two lists of digests part: the first zone that is not the same in both.
fn first_zone(ours: &Value, theirs: &Value) -> String {
    let (ours, theirs) = (ours.as_array().unwrap(), theirs.as_array().unwrap());
    for (i, (a, b)) in ours.iter().zip(theirs).enumerate() {
        if a != b {
            return format!("zone {i}: ours {} with {} polygons, the key's {} with {}", a[0], a[1], b[0], b[1]);
        }
    }
    format!("{} zones, the key has {}", ours.len(), theirs.len())
}

/// What is wrong with one case's polygons, if anything.
fn check(n: usize, case: &Value, seed: u64, o: &Value, lv: &Level) -> Result<(), String> {
    let at = format!("case {n} (seed {seed} {o})");
    assert_eq!((case["seed"].as_u64(), &case["options"]), (Some(seed), o), "the two keys list the same cases");
    let planned = geometry::planned(lv, &cx::KIT).map_err(|e| format!("{at}: {e}"))?;
    let planned_rows: Vec<Value> = planned.iter().map(poly_row).collect();
    let (polys, lit) = geometry::finish(lv, &cx::KIT, planned).map_err(|e| format!("{at}: {e}"))?;
    let rows: Vec<Value> = polys.iter().map(poly_row).collect();
    let lit_rows = Value::Array(lit.iter().map(|l| json!([l.room, l.count])).collect());
    if let Ok(dir) = std::env::var("AUTOLEVEL_POLYS_OUT") {
        // and, beyond what the key holds: each polygon's projected UVs and the beams' strength on
        // the polygons they light, as the floats' own bits (a zero keeps its sign)
        let raw = |x: f64| x.to_bits();
        let projected: Vec<Vec<u64>> = polys.iter().map(|p| csg::project_uvs(p).iter().flat_map(|t| [raw(t.0), raw(t.1)]).collect()).collect();
        let shine: Vec<Option<u64>> = polys.iter().map(|p| p.lit.map(|b| raw(sunbeam::intensity(p, &lit[b].beam)))).collect();
        let text = serde_json::to_string(&json!({"lit": lit_rows, "planned": planned_rows, "rows": rows, "projected": projected, "intensity": shine})).unwrap();
        std::fs::write(format!("{dir}/case{n}.json"), text).map_err(|e| format!("{dir}: {e}"))?;
    }
    if let Some(theirs) = case.get("rows") {
        let theirs = theirs.as_array().unwrap();
        for (i, (a, b)) in rows.iter().zip(theirs).enumerate() {
            if a != b {
                return Err(format!(
                    "{at}: polygon {i} of {} (the key has {}), in {}\n  ours  {}\n  key's {}\n  ours  {a}\n  key's {b}",
                    rows.len(),
                    theirs.len(),
                    b[0],
                    show(a),
                    show(b)
                ));
            }
        }
    }
    let planned_digests = zone_digests(&planned_rows);
    if planned_digests != case["planned"] {
        return Err(format!("{at}: after plan_polys, {}", first_zone(&planned_digests, &case["planned"])));
    }
    let digests = zone_digests(&rows);
    if digests != case["zones"] {
        return Err(format!("{at}: at the end, {}", first_zone(&digests, &case["zones"])));
    }
    let triangles: usize = polys.iter().map(|p| p.verts.len() - 2).sum();
    if json!([rows.len(), triangles, lit_rows]) != json!([case["count"], case["triangles"], case["lit"]]) {
        return Err(format!(
            "{at}: {} polygons, {triangles} triangles, lit {lit_rows}; the key has {}, {}, {}",
            rows.len(),
            case["count"],
            case["triangles"],
            case["lit"]
        ));
    }
    Ok(())
}

#[test]
fn polygons_are_the_pythons() {
    let key = key("polys");
    let cases = key.as_array().unwrap();
    let levels = levels();
    assert_eq!(cases.len(), levels.len());
    assert!(cases.len() >= 20 && cases[0].get("rows").is_some(), "the key has {} cases", cases.len());
    let wrong: Vec<String> = par(levels, |n, (seed, o, lv)| check(n, &cases[n], *seed, o, lv)).into_iter().filter_map(Result::err).collect();
    assert!(wrong.is_empty(), "{} of {} cases differ:\n{}", wrong.len(), cases.len(), wrong.join("\n"));
}

#[test]
fn lights_are_the_pythons() {
    let key = key("lights");
    let mut lamps = 0;
    for (case, (seed, o, lv)) in key.as_array().unwrap().iter().zip(levels()) {
        let ours: Vec<Value> = cx::plan_lights(&lv.plan, None, 0.4)
            .iter()
            .map(|l| {
                assert!(l.color[0] == l.color[1] && l.color[1] == l.color[2]);
                json!([fbits(l.pos[0]), fbits(l.pos[1]), fbits(l.pos[2]), fbits(l.color[0]), fbits(l.radius), fbits(l.size), fbits(l.spot)])
            })
            .collect();
        let theirs = case["lights"].as_array().unwrap();
        for (i, (a, b)) in ours.iter().zip(theirs).enumerate() {
            let f = |v: &Value| v.as_array().unwrap().iter().map(|x| f64::from_bits(x.as_u64().unwrap())).collect::<Vec<_>>();
            assert_eq!(a, b, "seed {seed} {o}: light {i}: ours {:?}, the key's {:?}", f(a), f(b));
        }
        assert_eq!(ours.len(), theirs.len(), "seed {seed} {o}: lights");
        lamps += ours.len();
    }
    assert!(lamps > 5000, "the key has {lamps} lights");
}
