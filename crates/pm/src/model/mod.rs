pub mod package;
pub mod publish_payload;

/// Whether to perform the real operation or just simulate it.
///
/// Used by both `pack` and `publish` commands to gate side-effects
/// (writing tarballs, sending registry requests) behind a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    DryRun,
    Live,
}

impl From<bool> for RunMode {
    /// `true` (CLI `--dry-run` flag present) → `DryRun`.
    fn from(dry_run: bool) -> Self {
        if dry_run { Self::DryRun } else { Self::Live }
    }
}
