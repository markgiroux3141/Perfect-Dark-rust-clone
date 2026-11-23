//! The slow work, on a thread, so the window keeps drawing: making a recipe's level (`Job`:
//! under a second in a release build, several in a debug one) and finding what could be joined
//! to a picked room (`Search`: a level tried for every corridor found, a second or two). As
//! `pd_edit::jobs` in the clone: a task is spawned, polled each tick, and ends with the result
//! to swap in.

use crate::deadlist::{make, Made};
use crate::doc::Pick;
use autolevel::candidates::{candidates_asked, Candidates};
use autolevel::recipe::Recipe;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::Instant;

/// Something being worked out on a thread.
pub struct Task<T> {
    /// The document's count of changes when it was started (`Doc::rev`).
    pub rev: u64,
    pub started: Instant,
    rx: Receiver<Result<T, String>>,
}

impl<T: Send + 'static> Task<T> {
    /// Start `work` on a thread of its own.
    pub fn run(rev: u64, work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Task<T> {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            // (the generator has places where it stops dead, as the Python does: an error here)
            let result = catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| Err("the generator stopped on this recipe (see the terminal)".to_string()));
            let _ = tx.send(result);
        });
        Task { rev, started: Instant::now(), rx }
    }

    /// The result, once: None while it is still being worked out.
    pub fn poll(&self) -> Option<Result<T, String>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the generator's thread ended with nothing to show".to_string())),
        }
    }
}

/// A level being made.
pub type Job = Task<Made>;

impl Task<Made> {
    /// Start making `recipe`'s level.
    pub fn spawn(recipe: Recipe, rev: u64) -> Job {
        Task::run(rev, move || make(&recipe))
    }
}

/// What could be joined to a picked room, being found.
pub struct Search {
    /// The room and storey it is looking from, and what is asked of the joins.
    pub pick: Pick,
    task: Task<Candidates>,
}

impl Search {
    /// Start looking for the corridors that could be made from `pick` in `recipe`'s level.
    pub fn spawn(recipe: Recipe, pick: Pick, rev: u64) -> Search {
        let (room, storey, asked) = pick.clone();
        Search { pick, task: Task::run(rev, move || candidates_asked(&recipe, &room, storey, &asked)) }
    }

    /// The document's count of changes when it was started.
    pub fn rev(&self) -> u64 {
        self.task.rev
    }

    pub fn started(&self) -> Instant {
        self.task.started
    }

    /// The result, once: None while it is still looking.
    pub fn poll(&self) -> Option<Result<Candidates, String>> {
        self.task.poll()
    }
}
