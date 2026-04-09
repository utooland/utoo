pub mod package;
pub mod publish_payload;

/// Whether to perform the real operation or just simulate it.
///
/// Replaces bare `dry_run: bool` for readability.
/// See: <https://blakesmith.me/2019/05/07/rust-patterns-enums-instead-of-booleans.html>
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
