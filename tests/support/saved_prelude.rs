use std::path::PathBuf;

use rslisp::expand::PreludeState;

use super::{Fixture, FixtureReports};

pub struct SavedPreludeAgreement {
    paths: Vec<PathBuf>,
}

impl SavedPreludeAgreement {
    pub fn of(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }

    pub fn assert_loaded_prelude_agrees(&self) {
        let saved: &'static [u8] = PreludeState::build().save().leak();
        let built = FixtureReports::compute(self.paths.clone(), |path| {
            Fixture::load(path).stage_report()
        });
        let loaded = FixtureReports::compute(self.paths.clone(), |path| {
            let prelude = PreludeState::load(saved).expect("the saved prelude loads");
            Fixture::load(path)
                .with_saved_prelude(prelude)
                .stage_report()
        });
        for path in &self.paths {
            assert_eq!(
                loaded.report(path),
                built.report(path),
                "a loaded prelude changes {}",
                path.display()
            );
        }
    }
}
