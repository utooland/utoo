//! Process-wide CLI presentation and interactivity policy.

use std::env;
use std::io::{self, IsTerminal};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorPolicy {
    #[default]
    Auto,
    Never,
}

impl ColorPolicy {
    pub fn resolve(no_color: bool) -> Self {
        if no_color || env::var_os("NO_COLOR").is_some() {
            Self::Never
        } else {
            Self::Auto
        }
    }

    pub const fn ansi_enabled(self) -> bool {
        matches!(self, Self::Auto)
    }

    pub const fn clap_choice(self) -> clap::ColorChoice {
        match self {
            Self::Auto => clap::ColorChoice::Auto,
            Self::Never => clap::ColorChoice::Never,
        }
    }

    pub fn apply(self) {
        if self == Self::Never {
            colored::control::set_override(self.ansi_enabled());
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

impl From<bool> for OutputFormat {
    fn from(json: bool) -> Self {
        if json { Self::Json } else { Self::Human }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Invocation {
    pub output: OutputFormat,
    pub quiet: bool,
    pub color: ColorPolicy,
    pub command: Option<&'static str>,
}

static INVOCATION: OnceLock<Invocation> = OnceLock::new();

pub fn init(options: Invocation) {
    options.color.apply();
    INVOCATION
        .set(options)
        .expect("invocation policy must be initialized exactly once");
}

pub fn json() -> bool {
    INVOCATION
        .get()
        .is_some_and(|options| options.output == OutputFormat::Json)
}

pub fn quiet() -> bool {
    INVOCATION.get().is_some_and(|options| options.quiet)
}

pub fn color() -> ColorPolicy {
    INVOCATION
        .get()
        .map_or(ColorPolicy::Auto, |options| options.color)
}

pub fn command() -> Option<&'static str> {
    INVOCATION.get().and_then(|options| options.command)
}

pub fn interactive() -> bool {
    io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_no_color_disables_ansi() {
        let no_color = true;
        let policy = ColorPolicy::resolve(no_color);
        assert_eq!(policy, ColorPolicy::Never);
        assert!(!policy.ansi_enabled());
        assert_eq!(policy.clap_choice(), clap::ColorChoice::Never);
    }
}
