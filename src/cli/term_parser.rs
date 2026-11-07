use std::convert::Infallible;
use std::ffi::OsStr;

use clap::builder::{StringValueParser, TypedValueParser};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Arg, Command, Error};

use super::doc::DocCommand;
use crate::language::Language;

#[derive(Clone)]
pub struct TermParser;

impl TermParser {
    const LONG_FLAG_PREFIX: &str = "--";

    fn unknown_flag(command: &Command, flag: String) -> Error {
        let mut error = Self::unexpected(command, flag);
        error.insert(
            ContextKind::Suggested,
            ContextValue::StyledStrs(vec![Self::separator_tip().into()]),
        );
        error
    }

    fn separator_tip() -> String {
        format!(
            "to look up a term that starts with `{prefix}`, write `{} {} {prefix} <term>`",
            Language::NAME,
            DocCommand::NAME,
            prefix = Self::LONG_FLAG_PREFIX
        )
    }

    fn unexpected(command: &Command, argument: String) -> Error {
        let mut error = Error::new(ErrorKind::UnknownArgument).with_cmd(command);
        error.insert(ContextKind::InvalidArg, ContextValue::String(argument));
        error.insert(
            ContextKind::Usage,
            ContextValue::StyledStr(command.clone().render_usage()),
        );
        error
    }
}

impl TypedValueParser for TermParser {
    type Value = String;

    fn parse_ref(
        &self,
        command: &Command,
        argument: Option<&Arg>,
        value: &OsStr,
    ) -> Result<String, Error> {
        let term = StringValueParser::new().parse_ref(command, argument, value)?;
        if term.starts_with(Self::LONG_FLAG_PREFIX) {
            return Err(Self::unknown_flag(command, term));
        }
        Ok(term)
    }
}

#[derive(Clone)]
pub struct SurplusParser;

impl TypedValueParser for SurplusParser {
    type Value = Infallible;

    fn parse_ref(
        &self,
        command: &Command,
        _argument: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Infallible, Error> {
        Err(TermParser::unexpected(
            command,
            value.to_string_lossy().into_owned(),
        ))
    }
}
