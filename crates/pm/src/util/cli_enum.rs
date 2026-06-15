//! CLI parameter enums that replace bare `bool` or multi-flag patterns.
//!
//! Each enum models a CLI flag or flag group as a self-documenting type
//! instead of a raw `bool`. Constructed at the CLI boundary (main.rs)
//! and passed through the service layer.
//!
//! See: <https://blakesmith.me/2019/05/07/rust-patterns-enums-instead-of-booleans.html>
//! See: <https://github.com/rust-lang/book/issues/2186>

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveType {
    Dev,
    Peer,
    Optional,
    Prod,
}

impl SaveType {
    /// Pick the dependency section from the mutually-exclusive `--save-*`
    /// flags, defaulting to `Prod`. First flag set wins (dev > peer >
    /// optional), matching the CLI's documented precedence.
    pub fn from_flags(save_dev: bool, save_peer: bool, save_optional: bool) -> Self {
        if save_dev {
            Self::Dev
        } else if save_peer {
            Self::Peer
        } else if save_optional {
            Self::Optional
        } else {
            Self::Prod
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageAction {
    Add,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum OmitType {
    Dev,
    Optional,
    Peer,
}

/// Whether to run lifecycle scripts during install/rebuild.
///
/// Replaces bare `ignore_scripts: bool` for readability.
/// See: <https://blakesmith.me/2019/05/07/rust-patterns-enums-instead-of-booleans.html>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPolicy {
    Run,
    Ignore,
}

impl From<bool> for ScriptPolicy {
    fn from(ignore: bool) -> Self {
        if ignore { Self::Ignore } else { Self::Run }
    }
}

/// Whether a config operation targets the global (`~/.utoo/config.toml`)
/// or local (`.utoo.toml`) scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigScope {
    Local,
    Global,
}

impl From<bool> for ConfigScope {
    fn from(global: bool) -> Self {
        if global { Self::Global } else { Self::Local }
    }
}

/// Whether to perform the real operation or just simulate it.
/// Used by `pack` and `publish` to gate side-effects behind a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Live,
    DryRun,
}

impl From<bool> for RunMode {
    fn from(dry_run: bool) -> Self {
        if dry_run { Self::DryRun } else { Self::Live }
    }
}
