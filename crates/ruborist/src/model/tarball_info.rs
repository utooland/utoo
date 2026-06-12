//! Package tarball metadata for downloading and verification.

use crate::model::compatibility::{PlatformConstraint, is_platform_compatible};
use crate::model::manifest::CoreVersionManifest;

/// Package tarball information for downloading.
///
/// A lightweight structure containing only the fields needed for
/// downloading and verifying a package tarball. Uses references to
/// avoid cloning data from the source manifest.
#[derive(Debug, Clone, Copy)]
pub struct PackageTarballInfo<'a> {
    /// Package name
    pub name: &'a str,
    /// Resolved version
    pub version: &'a str,
    /// Tarball URL for downloading
    pub tarball_url: Option<&'a str>,
    /// Integrity hash for verification
    pub integrity: Option<&'a str>,
    /// OS compatibility constraint (if specified)
    pub os: Option<&'a PlatformConstraint>,
    /// CPU compatibility constraint (if specified)
    pub cpu: Option<&'a PlatformConstraint>,
}

impl PackageTarballInfo<'_> {
    /// Check if this package is compatible with the current platform (os + cpu).
    pub fn is_platform_compatible(&self) -> bool {
        is_platform_compatible(self.os, self.cpu)
    }
}

impl<'a> From<&'a CoreVersionManifest> for PackageTarballInfo<'a> {
    fn from(m: &'a CoreVersionManifest) -> Self {
        Self {
            name: &m.name,
            version: &m.version,
            tarball_url: m.dist.tarball.as_deref(),
            integrity: m.dist.integrity.as_deref(),
            os: m.os.as_ref(),
            cpu: m.cpu.as_ref(),
        }
    }
}
