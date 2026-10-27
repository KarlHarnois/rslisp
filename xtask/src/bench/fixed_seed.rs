use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::failure::BenchFailure;

pub struct FixedSeedLibrary<'a> {
    root: &'a Path,
}

impl<'a> FixedSeedLibrary<'a> {
    const SOURCE: &'static str = "xtask/src/bench/fixed_seed.c";
    const LIBRARY: &'static str = "target/bench/fixed_seed.so";
    const C_COMPILER: &'static str = "cc";
    const SHARED_LIBRARY: [&'static str; 3] = ["-shared", "-fPIC", "-o"];

    pub fn new(root: &'a Path) -> Self {
        Self { root }
    }

    pub fn build(&self) -> Result<PathBuf, BenchFailure> {
        let library = self.root.join(Self::LIBRARY);
        Self::create_parent(&library)?;
        let output = Command::new(Self::C_COMPILER)
            .args(Self::SHARED_LIBRARY)
            .arg(&library)
            .arg(self.root.join(Self::SOURCE))
            .output()
            .map_err(|error| BenchFailure::tool_missing(Self::C_COMPILER, error))?;
        if !output.status.success() {
            return Err(BenchFailure::build_failed(
                "the fixed-seed library",
                &output.stderr,
            ));
        }
        Ok(library)
    }

    fn create_parent(library: &Path) -> Result<(), BenchFailure> {
        let directory = library.parent().unwrap_or(library);
        fs::create_dir_all(directory).map_err(|error| BenchFailure::unwritable(directory, error))
    }
}
