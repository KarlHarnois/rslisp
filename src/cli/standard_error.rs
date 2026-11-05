use std::fmt::Display;
use std::io::{self, Write};

pub struct StandardError;

impl StandardError {
    pub fn write_line(text: impl Display) {
        let _unreportable = writeln!(io::stderr().lock(), "{text}");
    }
}
