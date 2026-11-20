//! The 3D view's state and its egui, with no window and no GPU: the panels
//! are run through egui with pointer and key events (as `autolevel_edit`'s
//! are), the previews made on the view's own threads.

use std::path::Path;

use autolevel::recipe::Edit;
use autolevel_edit::planview;
use glam::Vec3;

use super::*;
use crate::preview::{cell, world};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets")
}

/// A folder of this test's own for its previews, empty.
fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pd_edit_space_test_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn left(root: &Path) -> usize {
    std::fs::read_dir(root).map_or(0, |d| d.count())
}

fn tiles(space: &Space) -> usize {
    space.preview().expect("a preview on show").level.geom.polys.len()
}

fn seed(n: u64) -> gen::Recipe {
    gen::Recipe::python(n)
}

fn with(edit: Edit) -> gen::Recipe {
    let mut r = seed(40);
    r.edits.push(edit);
    r
}

fn ring() -> gen::Recipe {
    with(Edit::Layout { room: "colossal".into(), layout: "ring".into() })
}

/// The view in a window 1500 by 950 that nobody opened, as `main.rs` lays it
/// out: its keys, its bar over the middle, and in the middle the 3D view's
/// words or (with the plan on show) what it draws over the plan.
struct Rig {
    space: Space,
    egui: egui::Context,
    res: Arc<WorldRes>,
    recipe: gen::Recipe,
    /// The room in hand in the generator's panel.
    room: Option<String>,
    /// The generator's level, for the plan.
    level: Option<autolevel::level::Level>,
    /// How the last frame laid the plan out, and the fills of the shapes it drew.
    fit: Option<Fit>,
    fills: Vec<(egui::Color32, Vec<egui::Pos2>)>,
    root: PathBuf,
}

impl Rig {
    fn new(name: &str) -> Rig {
        let root = root(name);
        let res = pd_import::autowalk::SimWalk::res(&assets()).expect("assets/");
        Rig { space: Space::new(assets(), root.clone()), egui: egui::Context::default(), res, recipe: seed(40), room: None, level: None, fit: None, fills: Vec::new(), root }
    }

    /// One frame with these events; the texts drawn in it.
    fn frame(&mut self, events: Vec<egui::Event>) -> Vec<String> {
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1500.0, 950.0));
        let Rig { space, egui, res, recipe, room, level, fit, .. } = self;
        let out = egui.run(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |egui| {
            space.keys(egui, Some(res));
            egui::TopBottomPanel::top("generator view").show(egui, |ui| space.bar(ui, recipe, room.as_deref(), Some(res)));
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(egui, |ui| {
                if space.middle == Middle::Space {
                    space.over_view(ui, recipe);
                } else {
                    // (the generator's editor lays its plan out like this, and leaves it in `fit`)
                    let rect = ui.max_rect();
                    *fit = level.as_ref().map(|lv| {
                        let (lo, hi) = planview::bounds(lv);
                        let f = Fit::inside(lo, hi, rect.width(), rect.height());
                        Fit { x: f.x + rect.min.x, y: f.y + rect.min.y, ..f }
                    });
                    space.over_plan(ui, rect, *fit, level.as_ref());
                }
            });
        });
        self.fills = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Path(p) => Some((p.fill, p.points.clone())),
                _ => None,
            })
            .collect();
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    fn click_at(&mut self, at: egui::Pos2, button: egui::PointerButton) {
        let press = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: Default::default() };
        self.frame(vec![egui::Event::PointerMoved(at)]);
        self.frame(vec![press(true)]);
        self.frame(vec![press(false)]);
    }

    /// Click one of the bar's controls, by its name.
    fn click(&mut self, name: &str) {
        self.frame(Vec::new());
        let at = self.space.spots.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no {name} in the bar: {:?}", self.space.spots)).1.center();
        self.click_at(at, egui::PointerButton::Primary);
    }

    /// A key goes down and up.
    fn key(&mut self, key: egui::Key) {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        self.frame(vec![event(true)]);
        self.frame(vec![event(false)]);
    }

    /// A right click on the plan at a place in cells.
    fn right_click_plan(&mut self, at: (f32, f32)) {
        self.frame(Vec::new());
        let (x, y) = self.fit.expect("the plan was laid out").to(at);
        self.click_at(egui::pos2(x, y), egui::PointerButton::Secondary);
    }

    /// Wait for the recipe's preview.
    fn settle(&mut self) -> bool {
        let recipe = self.recipe.clone();
        self.space.settle(&recipe)
    }

    /// Where the camera's mark was drawn last frame: the middle of its points.
    fn mark(&self) -> Option<egui::Pos2> {
        let pink = egui::Color32::from_rgba_unmultiplied(MARK[0], MARK[1], MARK[2], MARK[3]);
        let (_, pts) = self.fills.iter().find(|(fill, _)| *fill == pink)?;
        Some(egui::pos2(pts.iter().map(|p| p.x).sum::<f32>() / pts.len() as f32, pts.iter().map(|p| p.y).sum::<f32>() / pts.len() as f32))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn has(said: &[String], what: &str) -> bool {
    said.iter().any(|t| t.contains(what))
}

/// Is one of the texts exactly this (a button's)?
fn is(said: &[String], what: &str) -> bool {
    said.iter().any(|t| t == what)
}

#[test]
fn the_3d_view_follows_the_recipe_and_keeps_the_last_level_while_it_is_made_again() {
    let root = root("follow");
    let mut space = Space::new(assets(), root.clone());
    // with the plan on show and the 3D view never opened, nothing is made
    assert!(!space.follow(&seed(40)) && !space.busy() && space.preview().is_none() && left(&root) == 0);
    assert_eq!(space.status(&seed(40)), ("no 3D view yet".to_string(), false));
    // opened: the level's preview is made on a thread, and comes on show
    space.middle = Middle::Space;
    assert!(!space.follow(&seed(40)) && space.busy() && space.preview().is_none());
    assert!(space.status(&seed(40)).0.starts_with("making the 3D view... "));
    assert!(space.settle(&seed(40)), "it arrives: the window then loads its BG");
    assert!(space.current(&seed(40)) && !space.busy() && space.made == 1 && tiles(&space) == 12940);
    let line = space.preview().unwrap().line();
    assert_eq!(space.status(&seed(40)), (line, false));
    // the first preview places the camera: in the biggest hall, at a door
    assert!((space.cam.pos - world((21.5, 15.5), EYE)).length() < 0.01);
    // nothing more is made while the recipe stays
    assert!(!space.follow(&seed(40)) && !space.busy());

    // The camera is flown somewhere, and the recipe edited (the hall takes a ring of catwalks):
    // the last level stays on show while the new one is made, and the bar says so.
    space.cam = FlyCam::new(Vec3::new(1000.0, 400.0, -3000.0), 1.0, -0.2);
    assert!(!space.follow(&ring()) && space.busy() && !space.current(&ring()));
    let (text, warn) = space.status(&ring());
    assert!(text.starts_with("bringing the 3D view up to the plan... ") && text.ends_with("(the last level stays)") && warn, "{text}");
    assert_eq!(tiles(&space), 12940);
    assert!(space.settle(&ring()));
    assert!(space.current(&ring()) && space.made == 2 && tiles(&space) == 13328, "the ring's decks and ramps are in it");
    // the camera is where it was
    assert_eq!((space.cam.pos, space.cam.yaw, space.cam.pitch), (Vec3::new(1000.0, 400.0, -3000.0), 1.0, -0.2));
    // one scratch folder: the old preview's went with it
    assert_eq!(left(&root), 1);

    // A recipe that makes no level (seed 25): the last one stays, with the generator's reason.
    assert!(!space.settle(&seed(25)));
    assert_eq!(space.status(&seed(25)), ("this recipe makes no level, so the last one stays: seed 25: colossal: no set of ramps joins its storeys".to_string(), true));
    assert!(space.made == 2 && tiles(&space) == 13328 && left(&root) == 1);
    // it is not tried again while the recipe is that one
    assert!(!space.follow(&seed(25)) && !space.busy());
    // undone: the level on show is the recipe's own again, and nothing is made
    assert!(!space.follow(&ring()) && !space.busy() && space.current(&ring()));
    assert!(!space.status(&ring()).1);

    // A change while one is being made: the one under way comes on show, then the last asked for.
    let moved = with(Edit::Move { room: "medium1".into(), to: (60, 29) });
    space.follow(&seed(40));
    assert!(space.busy());
    assert!(space.settle(&moved));
    assert!(space.current(&moved) && space.made == 4, "seed 40's own first, then the move's: {}", space.made);

    // With the plan back on show the view is still kept up, so it is ready when gone back to.
    space.middle = Middle::Plan;
    assert!(space.wanted() && space.settle(&seed(40)) && space.current(&seed(40)));
    drop(space);
    assert_eq!(left(&root), 0, "the last preview's folder goes with the view");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_bar_switches_to_3d_and_walk_g_and_escape_go_between_walking_and_flying() {
    let mut rig = Rig::new("walk");
    // the plan is what opens; the bar offers the 3D view
    let said = rig.frame(Vec::new());
    assert!(rig.space.middle == Middle::Plan && is(&said, "Plan") && is(&said, "3D") && has(&said, "3D: the level as the game draws it, to fly through and walk") && !is(&said, "Walk"), "{said:?}");
    rig.click("3d");
    assert_eq!(rig.space.middle, Middle::Space);
    // (the window's tick)
    rig.space.follow(&seed(40));
    let said = rig.frame(Vec::new());
    assert!(said.iter().filter(|t| t.starts_with("making the 3D view... ")).count() == 2, "in the bar and in the middle: {said:?}");
    assert!(rig.settle());
    let said = rig.frame(Vec::new());
    assert!(is(&said, "Walk") && has(&said, "61 rooms, 91 portals, 12940 tiles, made in ") && has(&said, "Right button: look, WASD: fly, Q/E down and up, Shift faster, wheel: speed. G: walk from here"), "{said:?}");
    assert!(has(&said, "(2539, 159, -1831) yaw 213° pitch 0° · cell 21.5, 15.5 in colossal · speed 800 cm/s"), "{said:?}");

    // "Walk": a player on the floor under the camera, and the view is the player's.
    rig.click("walk");
    assert!(rig.space.walking());
    let said = rig.frame(Vec::new());
    assert!(is(&said, "Stop walking") && !is(&said, "Walk") && has(&said, "Walking: WASD or the arrows (left and right turn), right button: look, C: crouch a level down, Space: up, Esc: fly again"), "{said:?}");
    assert!(has(&said, "walking: feet (2539, 0, -1831) θ 147, standing · cell 21.5, 15.5 in colossal") && has(&said, "walking from feet (2539, 0, -1831) θ 147, standing"), "{said:?}");
    // the keys walk it; the fly camera stays where it was left
    let cam = rig.space.cam;
    for _ in 0..120 {
        rig.space.advance(&Motion { forward: 1.0, ..Motion::default() }, Controls { forward: 1.0, ..Controls::default() }, 1.0 / 60.0);
    }
    let feet = rig.space.walker.as_ref().unwrap().feet();
    assert!((feet - world((21.5, 15.5), 0.0)).length() > 600.0 && feet.y.abs() < 1.0 && rig.space.cam.pos == cam.pos);
    // the view drawn is from the player's eyes
    let eyes = rig.space.walker.as_ref().unwrap().p.pos;
    let view = rig.space.view(1.6, 15.0, 10000.0);
    assert!(view.eye == eyes && (view.aspect - 1.6).abs() < 1e-6 && rig.space.eye().pos == eyes);
    assert!((eyes.y - feet.y - EYE).abs() < 20.0);
    // G does nothing more while walking
    rig.key(egui::Key::G);
    assert!(rig.space.walking() && rig.space.walker.as_ref().unwrap().p.pos == eyes);

    // Escape: flying again, the camera at the player's eyes, looking as it did.
    rig.key(egui::Key::Escape);
    assert!(!rig.space.walking());
    assert!(rig.space.cam.pos == eyes && (rig.space.cam.yaw - cam.yaw).abs() < 0.01 && rig.space.cam.speed == cam.speed);
    let said = rig.frame(Vec::new());
    assert!(has(&said, "flying again, from the walker's eyes") && is(&said, "Walk") && !is(&said, "Stop walking"), "{said:?}");
    // and the keys fly it
    rig.space.advance(&Motion { forward: 1.0, ..Motion::default() }, Controls::default(), 0.5);
    assert!(((rig.space.cam.pos - eyes).length() - 400.0).abs() < 1.0);

    // G walks from there; "Stop walking" is Escape.
    rig.key(egui::Key::G);
    assert!(rig.space.walking());
    let w = rig.space.walker.as_ref().unwrap();
    assert!((w.feet().x - rig.space.cam.pos.x).abs() < 0.01 && (w.feet().z - rig.space.cam.pos.z).abs() < 0.01 && w.feet().y.abs() < 0.01);
    rig.click("stop");
    assert!(!rig.space.walking());

    // The wheel over the view sets the fly speed.
    let over = egui::pos2(900.0, 500.0);
    rig.frame(vec![egui::Event::PointerMoved(over)]);
    rig.frame(vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 50.0), modifiers: egui::Modifiers::NONE }]);
    assert!((rig.space.cam.speed - 800.0 * 1.15).abs() < 0.5, "{}", rig.space.cam.speed);

    // "Go to <room>" for the room in hand in the generator's panel: the camera over it.
    assert!(!has(&rig.frame(Vec::new()), "Go to"));
    rig.room = Some("loft1".to_string());
    assert!(has(&rig.frame(Vec::new()), "Go to loft1"));
    rig.click("go");
    let at = rig.space.cam.pos;
    assert_eq!(rig.space.preview().unwrap().room_at(at).map(|r| r.key.as_str()), Some("loft1"));
    assert!(has(&rig.frame(Vec::new()), "over loft1") && has(&rig.frame(Vec::new()), " in loft1 · speed"));

    // Outside the level there is no floor to walk on: it says so, and goes on flying.
    rig.space.cam.pos = Vec3::new(-3000.0, 300.0, 3000.0);
    rig.click("walk");
    assert!(!rig.space.walking());
    assert_eq!(rig.space.note, Some(("no floor under the camera: fly over a floor first".to_string(), true)));
    assert!(has(&rig.frame(Vec::new()), "no floor under the camera: fly over a floor first"));

    // With the plan on show the keys are not the 3D view's.
    rig.go_home();
    rig.click("plan");
    rig.key(egui::Key::G);
    assert!(rig.space.middle == Middle::Plan && !rig.space.walking());
}

impl Rig {
    /// The camera back at the hall's door.
    fn go_home(&mut self) {
        self.space.cam.pos = world((21.5, 15.5), EYE);
    }
}

#[test]
fn a_level_made_again_under_a_walker_puts_it_back_on_a_floor_or_ends_the_walk() {
    let mut rig = Rig::new("remade");
    rig.space.middle = Middle::Space;
    assert!(rig.settle());
    // a walker in the hall; the hall's ring is made around it: it stays where it stands
    rig.space.walk(Some(&rig.res.clone()));
    for _ in 0..90 {
        rig.space.advance(&Motion::default(), Controls { forward: 1.0, ..Controls::default() }, 1.0 / 60.0);
    }
    let (feet, note) = (rig.space.walker.as_ref().unwrap().feet(), rig.space.note.clone());
    rig.recipe = ring();
    assert!(rig.settle() && tiles(&rig.space) == 13328);
    assert!(rig.space.walking() && rig.space.walker.as_ref().unwrap().feet() == feet && rig.space.note == note);
    // and walks on, on the new level's tiles
    for _ in 0..60 {
        rig.space.advance(&Motion::default(), Controls { forward: 1.0, ..Controls::default() }, 1.0 / 60.0);
    }
    assert!((rig.space.walker.as_ref().unwrap().feet() - feet).length() > 200.0);

    // A walker in medium1, and the room is moved ten cells east from under it: the walker is put
    // on the nearest floor, and the view says how far that was.
    rig.space.stop_walking();
    assert!(rig.space.go_to_cell((53.5, 31.5), None));
    assert_eq!(rig.space.cam.pos, world((53.5, 31.5), EYE));
    rig.space.walk(Some(&rig.res.clone()));
    assert!(rig.space.walking());
    rig.recipe = with(Edit::Move { room: "medium1".into(), to: (60, 29) });
    assert!(rig.settle());
    let w = rig.space.walker.as_ref().expect("still walking");
    let feet = w.feet();
    let pv = rig.space.preview().unwrap();
    assert!((pv.floor_under(feet + Vec3::Y * 60.0).unwrap() - feet.y).abs() < 1.0, "on a floor of the new level: {}", w.line());
    let far = pv.lo - Vec3::splat(4000.0);
    let (note, wrong) = rig.space.note.clone().unwrap();
    assert!(note.starts_with("the floor under the walker is gone: it was put on the nearest, ") && note.ends_with(" m off") && !wrong, "{note}");
    assert!(has(&rig.frame(Vec::new()), "the floor under the walker is gone: it was put on the nearest, "));

    // A walker with no floor anywhere near in the new level: the walk ends, the camera is left at
    // its eyes, and the view says so.
    rig.space.walker = Some(Walker::at(rig.res.clone(), far, 90.0).unwrap());
    rig.recipe = seed(40);
    assert!(rig.settle());
    assert!(!rig.space.walking());
    assert!((rig.space.cam.pos - (far + Vec3::Y * EYE)).length() < 0.01);
    assert_eq!(rig.space.note, Some(("the floor the walker stood on is gone, and there is none near: the walk is over, the camera is where the walker's eyes were".to_string(), true)));
    let said = rig.frame(Vec::new());
    assert!(has(&said, "the walk is over") && is(&said, "Walk") && !is(&said, "Stop walking"), "{said:?}");
}

#[test]
fn the_camera_is_marked_on_the_plan_and_a_right_click_sends_it_there() {
    let mut rig = Rig::new("plan");
    rig.level = Some(seed(40).generate().unwrap());
    // nothing is marked before the 3D view has a camera
    rig.frame(Vec::new());
    assert!(rig.mark().is_none() && rig.fit.is_some());
    // a right click on nothing: no floor, and the plan stays
    rig.right_click_plan((8.3, 8.4));
    assert_eq!((rig.space.middle, rig.space.note.clone()), (Middle::Plan, Some(("no floor at cell 8, 8".to_string(), true))));
    // a right click in the basin, before any preview: the camera stands on that room's floor
    // (the generator's own level says where it is), and the 3D view comes on show and is made
    rig.right_click_plan((36.5, 44.0));
    assert_eq!(rig.space.middle, Middle::Space);
    let at = cell(rig.space.cam.pos);
    assert!((at.0 - 36.5).abs() < 0.05 && (at.1 - 44.0).abs() < 0.05 && rig.space.cam.pos.y == EYE, "{:?}", rig.space.cam.pos);
    assert!(rig.space.wanted() && !rig.space.follow(&seed(40)) && rig.space.busy());
    assert!(rig.settle());
    // (the first preview leaves a camera that has been put somewhere where it is)
    assert!((cell(rig.space.cam.pos).0 - 36.5).abs() < 0.05);

    // Back on the plan: the camera's mark is where the camera is.
    rig.click("plan");
    rig.frame(Vec::new());
    let (mark, fit) = (rig.mark().expect("the camera's mark"), rig.fit.unwrap());
    let want = fit.to(cell(rig.space.cam.pos));
    assert!((mark.x - want.0).abs() < 6.0 && (mark.y - want.1).abs() < 6.0, "{mark:?} for {want:?}");
    // turned to face east, its point is east of its middle
    rig.space.cam.yaw = std::f32::consts::FRAC_PI_2;
    rig.frame(Vec::new());
    let pink = egui::Color32::from_rgba_unmultiplied(MARK[0], MARK[1], MARK[2], MARK[3]);
    let tip = rig.fills.iter().find(|(fill, _)| *fill == pink).unwrap().1[0];
    assert!((tip.x - (want.0 + MARK_PX)).abs() < 0.01 && (tip.y - want.1).abs() < 0.01, "{tip:?}");

    // With a preview the floor is the level's own tiles: from above, onto the hall's catwalk at 551;
    // from the floor, under it.
    rig.space.cam.pos.y = 900.0;
    rig.right_click_plan((22.5, 15.5));
    assert!(rig.space.middle == Middle::Space && (rig.space.cam.pos.y - (551.0 + EYE)).abs() < 0.01, "{:?}", rig.space.cam.pos);
    rig.click("plan");
    rig.space.cam.pos.y = 100.0;
    rig.right_click_plan((22.5, 15.5));
    assert!((rig.space.cam.pos.y - EYE).abs() < 0.01);
    // a walker's place is what is marked while walking, and a right click ends the walk
    rig.space.walk(Some(&rig.res.clone()));
    assert!(rig.space.walking());
    rig.click("plan");
    rig.frame(Vec::new());
    let (mark, want) = (rig.mark().unwrap(), fit.to((22.5, 15.5)));
    assert!((mark.x - want.0).abs() < 6.0 && (mark.y - want.1).abs() < 6.0);
    rig.right_click_plan((36.5, 44.0));
    assert!(!rig.space.walking() && (cell(rig.space.cam.pos).0 - 36.5).abs() < 0.05);
}
