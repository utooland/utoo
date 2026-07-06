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

/// Registry visibility for a published package (npm `--access`).
///
/// Mirrors `npm publish --access <public|restricted>`. utoo historically only
/// supported `public`; `restricted` is accepted for scoped packages on
/// registries that support private publishing.
///
/// Single source of truth — strum derives the enum↔string conversions; the
/// `public`/`restricted` spellings come from the variant names via
/// `serialize_all` (and clap's `rename_all` for the `--access` flag).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, strum::EnumString, strum::IntoStaticStr,
)]
#[clap(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum PublishAccess {
    Public,
    Restricted,
}
