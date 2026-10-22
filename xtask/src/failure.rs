use std::io;
use std::path::Path;
use std::process::{ExitCode, Output};

pub struct GateFailure {
    message: String,
}

impl GateFailure {
    pub fn tool_missing(error: io::Error) -> Self {
        Self {
            message: format!(
                "cannot run `cargo llvm-cov`: {error}. Run the gate inside `nix develop`."
            ),
        }
    }

    pub fn clippy_missing(error: io::Error) -> Self {
        Self {
            message: format!(
                "cannot run `clippy-driver`: {error}. Run the gate inside `nix develop`."
            ),
        }
    }

    pub fn tests_failed(output: &Output) -> Self {
        Self {
            message: format!(
                "the instrumented test run failed\n{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }

    pub fn unreadable(path: &Path, error: io::Error) -> Self {
        Self {
            message: format!("cannot read `{}`: {error}", path.display()),
        }
    }

    pub fn unwritable(path: &Path, error: io::Error) -> Self {
        Self {
            message: format!("cannot create `{}`: {error}", path.display()),
        }
    }

    pub fn unparsable(path: &Path, error: &syn::Error) -> Self {
        Self {
            message: format!("cannot parse `{}`: {error}", path.display()),
        }
    }

    pub fn report(&self) -> ExitCode {
        eprintln!("error: {}", self.message);
        ExitCode::FAILURE
    }
}
