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

/// Whether dependencies are installed into the project or global prefix.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum InstallScope {
    #[default]
    Local,
    Global,
}

impl InstallScope {
    pub const fn is_global(self) -> bool {
        matches!(self, Self::Global)
    }

    pub const fn as_env_value(self) -> &'static str {
        match self {
            Self::Global => "true",
            Self::Local => "",
        }
    }
}

impl From<bool> for InstallScope {
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

/// How a destructive command obtains user confirmation.
///
/// Constructed from the CLI flag and validated against terminal interactivity
/// at the CLI boundary, so commands never receive a context-free boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationPolicy {
    Prompt,
    AssumeYes,
}

impl ConfirmationPolicy {
    pub const fn requires_interaction(self) -> bool {
        matches!(self, Self::Prompt)
    }
}

impl From<bool> for ConfirmationPolicy {
    fn from(yes: bool) -> Self {
        if yes { Self::AssumeYes } else { Self::Prompt }
    }
}

/// Whether `init` prompts for package metadata or accepts all defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitMode {
    Interactive,
    Defaults,
}

impl InitMode {
    pub const fn requires_interaction(self) -> bool {
        matches!(self, Self::Interactive)
    }
}

impl From<bool> for InitMode {
    fn from(yes: bool) -> Self {
        if yes {
            Self::Defaults
        } else {
            Self::Interactive
        }
    }
}

/// Whether publishing generates and attaches a provenance attestation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenancePolicy {
    Skip,
    Generate,
}

impl ProvenancePolicy {
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Generate)
    }
}

impl From<bool> for ProvenancePolicy {
    fn from(provenance: bool) -> Self {
        if provenance {
            Self::Generate
        } else {
            Self::Skip
        }
    }
}

/// Console diagnostic level resolved from `--verbose` and `--quiet`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleVerbosity {
    Quiet,
    #[default]
    Normal,
    Verbose,
}

impl ConsoleVerbosity {
    /// Quiet takes precedence when both flags are supplied.
    pub const fn from_flags(verbose: bool, quiet: bool) -> Self {
        if quiet {
            Self::Quiet
        } else if verbose {
            Self::Verbose
        } else {
            Self::Normal
        }
    }
}

/// ANSI color behavior resolved from `--no-color`, JSON mode, and `NO_COLOR`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ColorPolicy {
    #[default]
    Auto,
    Never,
}

impl ColorPolicy {
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

impl From<bool> for ColorPolicy {
    fn from(no_color: bool) -> Self {
        if no_color { Self::Never } else { Self::Auto }
    }
}

/// Human or machine-readable command output.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_policies_resolve_at_the_cli_boundary() {
        let no_color = true;
        let no_color_not_requested = false;
        let verbose = true;
        let quiet_disabled = false;
        let quiet = true;

        assert_eq!(ColorPolicy::from(no_color), ColorPolicy::Never);
        assert_eq!(ColorPolicy::from(no_color_not_requested), ColorPolicy::Auto);
        assert_eq!(
            ConsoleVerbosity::from_flags(verbose, quiet_disabled),
            ConsoleVerbosity::Verbose
        );
        assert_eq!(
            ConsoleVerbosity::from_flags(verbose, quiet),
            ConsoleVerbosity::Quiet
        );
    }
}
