//! Process-wide CLI presentation and interactivity policy.

use std::env;
use std::io::{self, IsTerminal};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Default)]
pub struct Invocation {
    pub json: bool,
    pub quiet: bool,
    pub no_color: bool,
}

static INVOCATION: OnceLock<Invocation> = OnceLock::new();

pub fn init(options: Invocation) {
    configure_color(options.no_color || options.json);
    let _ = INVOCATION.set(options);
}

pub fn configure_color(no_color: bool) {
    if no_color || env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
    }
}

pub fn json() -> bool {
    INVOCATION.get().is_some_and(|options| options.json)
}

pub fn quiet() -> bool {
    INVOCATION.get().is_some_and(|options| options.quiet)
}

pub fn interactive() -> bool {
    io::stdin().is_terminal()
}
