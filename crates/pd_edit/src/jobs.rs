//! The slow work, on a thread: importing a level (a minute: the source, the
//! geometry, the waypoints), placing it again with the layout (a second or
//! two: the waypoints come from the cache), the check match. Each ends with
//! the stage loaded again, so the window only swaps it in.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Instant;

use pd_import::layout::Layout;
use pd_import::recipe::Recipe;
use pd_sim::world::WorldRes;

use crate::paths::EditPaths;
use crate::scene::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// The whole import: the source converted, the waypoints generated.
    Import,
    /// Placed again with the layout; `fresh`: the waypoints generated anew.
    Place { fresh: bool },
    /// A minute of simulants on the stage as it is.
    Check,
    /// GoldenEye's door models and their catalogue, from the ROM (`pd_import
    /// --ge-doors`).
    GeDoors,
}

impl Work {
    pub fn label(self) -> &'static str {
        match self {
            Work::Import => "Building the arena (up to a minute)",
            Work::Place { fresh: false } => "Saving",
            Work::Place { fresh: true } => "Making the simulant paths",
            Work::Check => "Running the simulant test",
            Work::GeDoors => "Extracting GoldenEye's doors",
        }
    }
}

pub struct Done {
    pub work: Work,
    /// The report, and the stage loaded again (none for a check).
    pub result: Result<(Vec<String>, Option<Scene>), String>,
    /// A check's numbers.
    pub stats: Option<pd_import::CheckStats>,
}

pub struct Job {
    pub work: Work,
    pub started: Instant,
    /// The layout it places.
    pub layout: Layout,
    rx: Receiver<Done>,
}

impl Job {
    pub fn spawn(work: Work, recipe: Recipe, layout: Layout, paths: EditPaths, res: Arc<WorldRes>, weapons: [u8; 6]) -> Job {
        let (tx, rx) = channel();
        let placed = layout.clone();
        std::thread::spawn(move || {
            let ip = paths.import_paths();
            let reload = |report: Vec<String>| -> Result<(Vec<String>, Option<Scene>), String> {
                let scene = Scene::load(&paths.asset_dir(), &recipe.code, res.clone(), weapons)?;
                Ok((report, Some(scene)))
            };
            let mut stats = None;
            let result = match work {
                Work::Import => pd_import::import(&recipe, &ip, &layout).and_then(reload),
                Work::Place { fresh } => pd_import::replace(&recipe, &ip, &layout, pd_import::Finish { check: false, fresh_graph: fresh }).and_then(reload),
                Work::Check => pd_import::check_stats(&paths.asset_dir(), &recipe.code).map(|(r, s)| {
                    stats = Some(s);
                    (r, None)
                }),
                Work::GeDoors => pd_import::doors::extract_ge(&ip, &paths.levels).map(|r| (r, None)),
            };
            let _ = tx.send(Done { work, result, stats });
        });
        Job { work, started: Instant::now(), layout: placed, rx }
    }

    pub fn poll(&self) -> Option<Done> {
        self.rx.try_recv().ok()
    }
}
