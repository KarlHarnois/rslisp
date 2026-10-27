use std::env;
use std::process::Command;

pub struct Cargo;

impl Cargo {
    const DEFAULT_PROGRAM: &'static str = "cargo";

    pub fn command() -> Command {
        Command::new(env::var_os("CARGO").unwrap_or_else(|| Self::DEFAULT_PROGRAM.into()))
    }
}
