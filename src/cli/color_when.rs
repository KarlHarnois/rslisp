use std::ffi::OsString;

use clap::ValueEnum;

#[derive(Clone, Copy, Default, ValueEnum)]
pub enum ColorWhen {
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorWhen {
    const FLAG: &str = "--color";
    const FLAG_WITH_VALUE: &str = "--color=";
    const END_OF_OPTIONS: &str = "--";

    pub fn written_in(arguments: &[OsString]) -> Self {
        let options: Vec<&str> = arguments
            .iter()
            .map(|argument| argument.to_str().unwrap_or_default())
            .take_while(|argument| *argument != Self::END_OF_OPTIONS)
            .collect();
        options
            .windows(2)
            .filter_map(Self::written_value)
            .next_back()
            .and_then(|value| Self::from_str(value, false).ok())
            .unwrap_or_default()
    }

    fn written_value<'a>(pair: &[&'a str]) -> Option<&'a str> {
        match pair {
            [Self::FLAG, value] => Some(value),
            [_, option] => option.strip_prefix(Self::FLAG_WITH_VALUE),
            _ => None,
        }
    }
}

impl From<ColorWhen> for clap::ColorChoice {
    fn from(when: ColorWhen) -> Self {
        match when {
            ColorWhen::Auto => Self::Auto,
            ColorWhen::Always => Self::Always,
            ColorWhen::Never => Self::Never,
        }
    }
}

impl From<ColorWhen> for anstream::ColorChoice {
    fn from(when: ColorWhen) -> Self {
        match when {
            ColorWhen::Auto => Self::Auto,
            ColorWhen::Always => Self::Always,
            ColorWhen::Never => Self::Never,
        }
    }
}
