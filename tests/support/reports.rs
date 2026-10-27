use std::collections::HashMap;
use std::env;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;

pub struct FixtureReports {
    reports: HashMap<PathBuf, String>,
}

impl FixtureReports {
    const JOB_LIMIT: &str = "CARGO_BUILD_JOBS";

    pub fn compute(paths: Vec<PathBuf>, report: impl Fn(&Path) -> String + Sync) -> Self {
        let pending = Mutex::new(paths);
        let finished = Mutex::new(HashMap::new());
        thread::scope(|scope| {
            for _ in 0..Self::workers() {
                scope.spawn(|| Self::drain(&pending, &finished, &report));
            }
        });
        Self {
            reports: finished.into_inner().expect("every worker finished"),
        }
    }

    pub fn report(&self, path: &Path) -> &str {
        self.reports
            .get(path)
            .expect("the first glob pass listed this fixture")
    }

    fn workers() -> usize {
        env::var(Self::JOB_LIMIT)
            .ok()
            .and_then(|limit| limit.parse().ok())
            .unwrap_or_else(|| thread::available_parallelism().map_or(1, NonZeroUsize::get))
    }

    fn drain(
        pending: &Mutex<Vec<PathBuf>>,
        finished: &Mutex<HashMap<PathBuf, String>>,
        report: &impl Fn(&Path) -> String,
    ) {
        while let Some(path) = Self::next(pending) {
            let text = report(&path);
            finished
                .lock()
                .expect("no worker panics while holding the reports")
                .insert(path, text);
        }
    }

    fn next(pending: &Mutex<Vec<PathBuf>>) -> Option<PathBuf> {
        pending
            .lock()
            .expect("no worker panics while holding the queue")
            .pop()
    }
}
