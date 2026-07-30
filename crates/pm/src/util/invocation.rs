//! Process-wide CLI presentation and interactivity policy.

use std::io::{self, IsTerminal};
use std::sync::{OnceLock, RwLock};
use std::time::Instant;

use super::cli_enum::{ColorPolicy, ConsoleVerbosity, OutputFormat};

#[derive(Debug, Clone, Copy)]
pub struct Invocation {
    pub output: OutputFormat,
    pub verbosity: ConsoleVerbosity,
    pub color: ColorPolicy,
    pub command: Option<&'static str>,
    pub subcommand: Option<&'static str>,
}

static INVOCATION: OnceLock<Invocation> = OnceLock::new();
static STARTED: OnceLock<Instant> = OnceLock::new();
static COMMAND: OnceLock<RwLock<(Option<&'static str>, Option<&'static str>)>> = OnceLock::new();

pub fn start() {
    STARTED
        .set(Instant::now())
        .expect("invocation timer must be initialized exactly once");
}

pub fn init(options: Invocation) {
    options.color.apply();
    COMMAND
        .set(RwLock::new((options.command, options.subcommand)))
        .expect("invocation command must be initialized exactly once");
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
    COMMAND
        .get()
        .and_then(|command| command.read().expect("invocation command lock poisoned").0)
}

pub fn subcommand() -> Option<&'static str> {
    COMMAND
        .get()
        .and_then(|command| command.read().expect("invocation command lock poisoned").1)
}

pub fn set_command(command: &'static str, subcommand: Option<&'static str>) {
    if let Some(current) = COMMAND.get() {
        *current.write().expect("invocation command lock poisoned") = (Some(command), subcommand);
    }
}

pub fn duration_ms() -> u64 {
    STARTED
        .get()
        .map_or(0, |started| started.elapsed().as_millis() as u64)
}

pub fn interactive() -> bool {
    !json() && io::stdin().is_terminal()
}
