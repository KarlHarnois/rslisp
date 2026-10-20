mod rustc_errors;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::diagnostic::Diagnostics;
use crate::emit::EmittedCrate;
use crate::source::Span;
use rustc_errors::RustcDiagnosticMapper;

pub enum BuildError {
    Program(Diagnostics),
    Environment(String),
}

pub struct RustCompiler {
    directory: PathBuf,
}

impl RustCompiler {
    const RUSTC: &str = "rustc";
    const SOURCE_FILE: &str = "main.rs";
    const EDITION: &str = "2024";
    const OPTIMIZE: &str = "opt-level=3";

    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn compile(
        &self,
        emitted: &EmittedCrate,
        crate_name: &str,
        output: &Path,
        release: bool,
        entry: Span,
    ) -> Result<(), BuildError> {
        let source = self
            .write_source(emitted)
            .map_err(|error| Self::environment("write the generated Rust", &error))?;
        Self::create_output_directory(output)?;
        let mut command = Command::new(Self::RUSTC);
        command
            .args([
                "--edition",
                Self::EDITION,
                "--crate-name",
                crate_name,
                "--error-format=json",
                "--cap-lints",
                "allow",
            ])
            .arg("-o")
            .arg(output)
            .arg(&source);
        if release {
            command.args(["-C", Self::OPTIMIZE]);
        }
        let result = command
            .output()
            .map_err(|error| Self::environment("run rustc", &error))?;
        if result.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&result.stderr);
        Err(BuildError::Program(
            RustcDiagnosticMapper::new(emitted, entry).map(&stderr),
        ))
    }

    fn write_source(&self, emitted: &EmittedCrate) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.directory)?;
        let source = self.directory.join(Self::SOURCE_FILE);
        fs::write(&source, &emitted.code)?;
        Ok(source)
    }

    fn create_output_directory(output: &Path) -> Result<(), BuildError> {
        let Some(directory) = output.parent() else {
            return Ok(());
        };
        fs::create_dir_all(directory).map_err(|error| {
            Self::environment(&format!("create `{}`", directory.display()), &error)
        })
    }

    fn environment(action: &str, error: &io::Error) -> BuildError {
        BuildError::Environment(format!("cannot {action}: {error}"))
    }
}
