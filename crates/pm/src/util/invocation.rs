//! Process-wide CLI presentation and interactivity policy.

use std::io::{self, IsTerminal};
use std::sync::OnceLock;

use super::cli_enum::{ColorPolicy, ConsoleVerbosity, OutputFormat};

#[derive(Debug, Clone, Copy)]
pub struct Invocation {
    pub output: OutputFormat,
    pub verbosity: ConsoleVerbosity,
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
    INVOCATION
        .get()
        .is_some_and(|options| options.verbosity == ConsoleVerbosity::Quiet)
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
