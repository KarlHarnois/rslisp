use std::env;
use std::process::Command;

use crate::failure::TaskFailure;

pub struct Cargo;

impl Cargo {
    const DEFAULT_PROGRAM: &'static str = "cargo";

    pub fn command() -> Command {
        Command::new(env::var_os("CARGO").unwrap_or_else(|| Self::DEFAULT_PROGRAM.into()))
    }

    pub fn run(
        command: &mut Command,
        failure: impl FnOnce(&[u8]) -> TaskFailure,
    ) -> Result<Vec<u8>, TaskFailure> {
        let output = command
            .output()
            .map_err(|error| TaskFailure::tool_missing(Self::DEFAULT_PROGRAM, error))?;
        if !output.status.success() {
            return Err(failure(&output.stderr));
        }
        Ok(output.stdout)
    }
}
