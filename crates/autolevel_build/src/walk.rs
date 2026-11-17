//! Walking a built level, and looking at it: a level's routes and views (written in plan cells)
//! given to something that moves as the game's player does. From `scripts/plan_walks.py`.
//!
//! The walking itself is behind a trait, `Walker`: something opened on one built level that
//! walks a route on it and renders a view of it. Two things implement it. `PdWalk`, here, runs
//! pd-walk (`viewer/` in the Blender MCP repo this started in: the game's movement and collision
//! as a program on the level's OBJ) once for each route. `pd_import::autowalk::SimWalk` walks in
//! this process, on `pd_sim`'s own player and the tiles the importer makes of the level; it is
//! what the `autolevel` command line uses unless pd-walk is named.
//!
//! A route is points `(x cell, y cell, feet z)` with flags: 1 = passed through on the move, 2 =
//! squat on the way there (a vent), 3 = both; a plain point is stopped at. The first point is
//! the spawn. A route whose name starts with `!` must fail to be walked (glass, water).
//! Routes are independent, so `walk_all` walks them on a few threads, and gives the outcomes in
//! the routes' own order.

use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Units to a plan cell.
pub const M: f64 = 118.11;

/// A point of a route: (x, y) in cells, the height of the feet there, and its flags (0: stop here).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub flags: u8,
}

/// A place to look from: stand at cell (x, y), feet at z, facing cell (tx, ty).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub tx: f64,
    pub ty: f64,
}

/// Plan cells (x, y) and the feet's height as the walker's coordinates, the game's: (x, z, -y).
pub fn ge(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    (x * M, z, -y * M)
}

/// A route as pd-walk's `--route` takes it: `x,y,z[,flags];...` in whole units.
pub fn route_string(pts: &[Point]) -> String {
    let one = |p: &Point| {
        let (gx, gy, gz) = ge(p.x, p.y, p.z);
        let flags = if p.flags != 0 { format!(",{}", p.flags) } else { String::new() };
        format!("{gx:.0},{gy:.0},{gz:.0}{flags}")
    };
    pts.iter().map(one).collect::<Vec<_>>().join(";")
}

/// A view as pd-walk's `--spawn` takes it: `x,y,z,theta`, the facing in degrees.
pub fn spawn_string(v: &View) -> String {
    let (gx, gy, gz) = ge(v.x, v.y, v.z);
    let theta = (-(v.tx - v.x)).atan2(-(v.ty - v.y)).to_degrees();
    format!("{gx:.0},{gy:.0},{gz:.0},{theta:.1}")
}

/// A route from JSON: `[[x, y, z], [x, y, z, flags], ...]`, as walks.json holds it.
pub fn route_from(v: &Value) -> Result<Vec<Point>, String> {
    let pts = v.as_array().ok_or("a route is a list of points")?;
    pts.iter()
        .map(|p| {
            let n: Vec<f64> = p.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            match (n.len(), p.as_array().map_or(0, Vec::len)) {
                (3, 3) => Ok(Point { x: n[0], y: n[1], z: n[2], flags: 0 }),
                (4, 4) => Ok(Point { x: n[0], y: n[1], z: n[2], flags: n[3] as u8 }),
                _ => Err(format!("a point is [x, y, z] or [x, y, z, flags], not {p}")),
            }
        })
        .collect()
}

/// A view from JSON: `[x, y, z, tx, ty]`, as views.json holds it.
pub fn view_from(v: &Value) -> Result<View, String> {
    let n: Vec<f64> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    match (n.len(), v.as_array().map_or(0, Vec::len)) {
        (5, 5) => Ok(View { x: n[0], y: n[1], z: n[2], tx: n[3], ty: n[4] }),
        _ => Err(format!("a view is [x, y, z, look at x, look at y], not {v}")),
    }
}

/// How a route was walked.
#[derive(Clone, Debug, PartialEq)]
pub struct Walked {
    /// Every point was reached.
    pub reached: bool,
    /// What the walker says of each waypoint after the first, in order (pd-walk: `i`, `reached`,
    /// `dist`, `feet`, `want`, `room`).
    pub reports: Vec<Value>,
}

/// Something that moves through one built level as the game's player does. It is opened on
/// the level (so what it loads, it loads once) and then given routes and views, from any thread.
pub trait Walker: Sync {
    /// Walk a route. Err if the level could not be walked at all (which is not the same as a
    /// route that stops short).
    fn walk(&self, route: &[Point]) -> Result<Walked, String>;

    /// Render what is seen from `view` to a PNG. Err, saying what does, if this walker has no
    /// renderer.
    fn shot(&self, view: &View, png: &Path) -> Result<(), String>;
}

/// The built level in a folder: its `LevelIndices.obj`. Err, saying so, if there is none.
pub fn level_obj(dir: &Path) -> Result<PathBuf, String> {
    let obj = dir.join("LevelIndices.obj");
    if obj.is_file() {
        Ok(obj)
    } else {
        Err(format!("no built level in {} (no LevelIndices.obj): autolevel build SEED --out {0}", dir.display()))
    }
}

/// pd-walk on one level's OBJ, run as a program once for each route or view.
#[derive(Clone, Debug, PartialEq)]
pub struct PdWalk {
    pub exe: PathBuf,
    /// The level: its `LevelIndices.obj`.
    pub obj: PathBuf,
    /// The longest one route may take, and one view.
    pub walk_timeout: Duration,
    pub shot_timeout: Duration,
}

impl PdWalk {
    /// Is pd-walk at `exe`? Err, saying how to get it, if there is no such file.
    pub fn found(exe: &Path) -> Result<(), String> {
        if exe.is_file() {
            return Ok(());
        }
        Err(format!(
            "pd-walk is not at {}: build it (cargo build --release, in the Blender MCP repo's viewer/), and name it with --pd-walk PATH or in the environment as PD_WALK",
            exe.display()
        ))
    }

    /// pd-walk at `exe`, on the level built in `dir`. Err if either is not there.
    pub fn open(exe: &Path, dir: &Path) -> Result<PdWalk, String> {
        PdWalk::found(exe)?;
        Ok(PdWalk { exe: exe.to_path_buf(), obj: level_obj(dir)?, walk_timeout: Duration::from_secs(300), shot_timeout: Duration::from_secs(120) })
    }

    /// Run pd-walk on the level: (its exit code, what it printed, what it said went wrong).
    fn run(&self, args: &[&str], timeout: Duration) -> Result<(i32, String, String), String> {
        let obj = &self.obj;
        let mut child = Command::new(&self.exe)
            .arg(obj)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", self.exe.display()))?;
        // its output is read as it comes, so a full pipe never holds it up while it is waited for
        let (mut out, mut err) = (child.stdout.take().expect("stdout is piped"), child.stderr.take().expect("stderr is piped"));
        let read_out = std::thread::spawn(move || {
            let mut s = String::new();
            out.read_to_string(&mut s).map(|_| s)
        });
        let read_err = std::thread::spawn(move || {
            let mut s = String::new();
            err.read_to_string(&mut s).map(|_| s)
        });
        let until = Instant::now() + timeout;
        let status = loop {
            match child.try_wait().map_err(|e| format!("{}: {e}", self.exe.display()))? {
                Some(status) => break status,
                None if Instant::now() >= until => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("pd-walk took more than {} s on {}", timeout.as_secs(), obj.display()));
                }
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        };
        let text = |h: std::thread::JoinHandle<std::io::Result<String>>| h.join().ok().and_then(Result::ok).unwrap_or_default();
        Ok((status.code().unwrap_or(-1), text(read_out), text(read_err)))
    }
}

/// What pd-walk said went wrong: its own lines, the log's left out.
fn complaint(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().filter(|l| l.starts_with("pd-walk:")).collect();
    if lines.is_empty() {
        stderr.trim().to_string()
    } else {
        lines.join("; ")
    }
}

/// pd-walk's reports: the lines of its output that are JSON objects.
pub fn reports(stdout: &str) -> Vec<Value> {
    stdout.lines().filter(|l| l.starts_with('{')).filter_map(|l| serde_json::from_str(l).ok()).collect()
}

impl Walker for PdWalk {
    fn walk(&self, route: &[Point]) -> Result<Walked, String> {
        let (code, out, err) = self.run(&["--route", &route_string(route)], self.walk_timeout)?;
        match code {
            // 0: every point reached. 1: it stopped short.
            0 | 1 => Ok(Walked { reached: code == 0, reports: reports(&out) }),
            _ => Err(format!("pd-walk could not walk {} (exit {code}): {}", self.obj.display(), complaint(&err))),
        }
    }

    fn shot(&self, view: &View, png: &Path) -> Result<(), String> {
        let png_text = png.to_string_lossy();
        let (code, _, err) = self.run(&["--spawn", &spawn_string(view), "--shot", &png_text], self.shot_timeout)?;
        match code {
            0 => Ok(()),
            _ => Err(format!("pd-walk rendered no frame (exit {code}): {}", complaint(&err))),
        }
    }
}

/// A named route, walked.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub name: String,
    /// It came out as it should: walked, or for a route named `!...`, not walked.
    pub ok: bool,
    /// What the walker found (Err: the level could not be walked at all).
    pub walked: Result<Walked, String>,
}

impl Outcome {
    /// The first waypoint that was not reached, from 0 (the walker's reports leave out the spawn).
    pub fn stuck(&self) -> Option<usize> {
        let reports = &self.walked.as_ref().ok()?.reports;
        reports.iter().position(|d| d.get("reached").and_then(Value::as_bool) == Some(false))
    }
}

/// Must this route fail to be walked?
pub fn must_fail(name: &str) -> bool {
    name.starts_with('!')
}

/// `work(n)` for every n below `count`, on up to `threads` threads; the results in order.
pub fn parallel<T: Send>(count: usize, threads: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::with_capacity(count));
    std::thread::scope(|s| {
        for _ in 0..threads.clamp(1, count.max(1)) {
            s.spawn(|| loop {
                let n = next.fetch_add(1, Ordering::Relaxed);
                if n >= count {
                    break;
                }
                let one = work(n);
                done.lock().expect("no worker panicked holding the results").push((n, one));
            });
        }
    });
    let mut done = done.into_inner().expect("no worker panicked holding the results");
    done.sort_by_key(|(n, _)| *n);
    done.into_iter().map(|(_, one)| one).collect()
}

/// How many threads to work on when nobody says: one for each processor.
pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

/// Walk every route on a walker's level, on up to `threads` threads. The outcomes are in the routes' order.
pub fn walk_all(walker: &dyn Walker, routes: &[(String, Vec<Point>)], threads: usize) -> Vec<Outcome> {
    parallel(routes.len(), threads, |n| {
        let (name, pts) = &routes[n];
        let walked = walker.walk(pts);
        let ok = walked.as_ref().is_ok_and(|w| w.reached != must_fail(name));
        Outcome { name: name.clone(), ok, walked }
    })
}

/// Render every view of a walker's level into `dir` as `<name>.png`, on up to `threads` threads.
/// For each view, in order: its name, and the file or what went wrong.
pub fn shoot_all(walker: &dyn Walker, views: &[(String, View)], dir: &Path, threads: usize) -> Vec<(String, Result<PathBuf, String>)> {
    parallel(views.len(), threads, |n| {
        let (name, view) = &views[n];
        let png = dir.join(format!("{name}.png"));
        (name.clone(), walker.shot(view, &png).map(|()| png))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pt(x: f64, y: f64, z: f64, flags: u8) -> Point {
        Point { x, y, z, flags }
    }

    /// `plan_walks.route_string` on the same points gave these.
    #[test]
    fn a_route_is_the_text_pythons_route_string_gives() {
        // seed 40's first corridor
        let link = [pt(21.5, 12.5, 0.0, 0), pt(21.5, 14.0, 0.0, 1), pt(21.5, 15.5, 0.0, 0)];
        assert_eq!(route_string(&link), "2539,0,-1476;2539,0,-1654,1;2539,0,-1831");
        // a vent: squat (2), squat and pass through (3), down a storey
        let vent = [pt(34.5, 28.5, 0.0, 0), pt(33.5, 28.5, 0.0, 3), pt(32.5, 27.5, -276.0, 3), pt(32.5, 27.5, -276.0, 0)];
        assert_eq!(route_string(&vent), "4075,0,-3366;3957,0,-3366,3;3839,-276,-3248,3;3839,-276,-3248");
        // the plan's origin: Python writes -0 for y = 0, and so must this
        assert_eq!(route_string(&[pt(0.0, 0.0, 1102.0, 0), pt(-1.5, -2.25, -118.0, 2)]), "0,1102,-0;-177,-118,266,2");
        assert_eq!(route_string(&[]), "");
    }

    #[test]
    fn a_view_stands_and_faces_as_pythons_shot_does() {
        // plan_walks.shot: theta = degrees(atan2(-(tx - x), -(ty - y)))
        let v = View { x: 21.5, y: 15.5, z: 0.0, tx: 13.0, ty: 28.5 };
        assert_eq!(spawn_string(&v), "2539,0,-1831,146.8");
        let v = View { x: 22.5, y: 15.5, z: 551.0, tx: 22.5, ty: 10.0 };
        assert_eq!(spawn_string(&v), "2657,551,-1831,-0.0");
        let v = View { x: 10.0, y: 10.0, z: -276.0, tx: 20.0, ty: 10.0 };
        assert_eq!(spawn_string(&v), "1181,-276,-1181,-90.0");
    }

    #[test]
    fn routes_and_views_are_read_from_json() {
        let route = route_from(&json!([[21.5, 12.5, 0], [21.5, 14.0, 0, 1], [1, 2, -276, 3]])).unwrap();
        assert_eq!(route, [pt(21.5, 12.5, 0.0, 0), pt(21.5, 14.0, 0.0, 1), pt(1.0, 2.0, -276.0, 3)]);
        assert_eq!(route_from(&json!([[1, 2]])).unwrap_err(), "a point is [x, y, z] or [x, y, z, flags], not [1,2]");
        assert_eq!(route_from(&json!([[1, 2, "up"]])).unwrap_err(), "a point is [x, y, z] or [x, y, z, flags], not [1,2,\"up\"]");
        assert_eq!(route_from(&json!({})).unwrap_err(), "a route is a list of points");
        assert_eq!(view_from(&json!([21.5, 15.5, 0, 13.0, 28.5])).unwrap(), View { x: 21.5, y: 15.5, z: 0.0, tx: 13.0, ty: 28.5 });
        assert!(view_from(&json!([1, 2, 3])).is_err());
    }

    /// A walker with no level behind it: a route is walked if none of its points is below -100.
    /// `Dry(false)` is one whose level cannot be walked at all.
    struct Dry(bool);

    impl Walker for Dry {
        fn walk(&self, route: &[Point]) -> Result<Walked, String> {
            if !self.0 {
                return Err("no level".to_string());
            }
            let reports = route[1..].iter().enumerate().map(|(i, p)| json!({"i": i + 1, "reached": p.z > -100.0})).collect();
            Ok(Walked { reached: route.iter().all(|p| p.z > -100.0), reports })
        }

        fn shot(&self, view: &View, _: &Path) -> Result<(), String> {
            if view.z < 0.0 {
                Err("dark".to_string())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn routes_are_walked_on_threads_and_come_back_in_order() {
        let mut routes: Vec<(String, Vec<Point>)> = (0..40).map(|n| (format!("link_{n}"), vec![pt(0.0, 0.0, 0.0, 0), pt(n as f64, 0.0, 0.0, 0)])).collect();
        routes.push(("!pool".to_string(), vec![pt(0.0, 0.0, 0.0, 0), pt(1.0, 0.0, 0.0, 1), pt(2.0, 0.0, -118.0, 0)]));
        routes.push(("!dry".to_string(), vec![pt(0.0, 0.0, 0.0, 0), pt(2.0, 0.0, 0.0, 0)]));
        routes.push(("fall".to_string(), vec![pt(0.0, 0.0, 0.0, 0), pt(2.0, 0.0, -276.0, 0)]));
        let out = walk_all(&Dry(true), &routes, 6);
        assert_eq!(out.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), routes.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>());
        assert!(out[..40].iter().all(|o| o.ok && o.stuck().is_none()));
        // a route that must fail is as expected when it stops short, and wrong when it is walked
        assert_eq!((out[40].ok, out[40].stuck(), out[41].ok, out[42].ok, out[42].stuck()), (true, Some(1), false, false, Some(0)));
        // a level that can't be walked at all is nothing as expected, the must-fail routes too
        let none = walk_all(&Dry(false), &routes, 3);
        assert!(none.iter().all(|o| !o.ok && o.walked == Err("no level".to_string()) && o.stuck().is_none()));
        assert!(walk_all(&Dry(true), &[], 4).is_empty());
        let views = [
            ("up".to_string(), View { x: 0.0, y: 0.0, z: 276.0, tx: 1.0, ty: 1.0 }),
            ("down".to_string(), View { x: 0.0, y: 0.0, z: -276.0, tx: 1.0, ty: 1.0 }),
        ];
        let shots = shoot_all(&Dry(true), &views, Path::new("shots"), 2);
        assert_eq!(shots, [("up".to_string(), Ok(PathBuf::from("shots").join("up.png"))), ("down".to_string(), Err("dark".to_string()))]);
        assert_eq!(parallel(5, 0, |n| n * n), [0, 1, 4, 9, 16]);
    }

    #[test]
    fn pd_walks_output_is_read_and_a_missing_exe_is_said() {
        let out =
            "{\"i\": 1, \"reached\": true, \"dist\": 2.0, \"feet\": 0.0, \"want\": 0.0, \"room\": \"primary_Room0B\"}\nnoise\n{\"i\": 2, \"reached\": false}\n";
        let r = reports(out);
        assert_eq!((r.len(), r[0]["room"].as_str(), r[1]["reached"].as_bool()), (2, Some("primary_Room0B"), Some(false)));
        let log =
            "[2026-11-16T23:53:15Z INFO  pd_walk::level] x.obj: 15303 triangles\npd-walk: x.obj: The system cannot find the file specified. (os error 2)\n";
        assert_eq!(complaint(log), "pd-walk: x.obj: The system cannot find the file specified. (os error 2)");
        let why = PdWalk::open(Path::new("nowhere/pd-walk.exe"), Path::new("nowhere")).unwrap_err();
        assert!(why.starts_with("pd-walk is not at nowhere") && why.contains("--pd-walk PATH") && why.contains("PD_WALK"), "{why}");
        assert!(level_obj(Path::new("nowhere")).unwrap_err().starts_with("no built level in nowhere"));
        assert!(must_fail("!window_c26b_medium1") && !must_fail("link_c01"));
    }
}
