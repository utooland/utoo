//! Pure candidate matching for dependency reuse, before and after resolution.
//!
//! Graph traversal and override selection stay with the caller. These helpers
//! only inspect metadata already present on the candidate or resolved manifest.

use deno_semver::VersionReq;

use super::semver::{matches, normalize_spec};
use crate::model::graph::PackageNode;
use crate::model::manifest::CoreVersionManifest;
use crate::spec::{Protocol, SpecStr};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MatchResult {
    Match,
    NoMatch,
    NeedsResolution,
}

impl From<bool> for MatchResult {
    fn from(matches: bool) -> Self {
        if matches { Self::Match } else { Self::NoMatch }
    }
}

/// Check the requested spec against an existing package before resolution.
/// Preserve the lockfile reuse policy for ordinary dist-tag requests.
pub(crate) fn matches_spec(candidate: &PackageNode, spec: &str) -> bool {
    match Protocol::strip_prefix(spec) {
        // HTTP tarballs are identified by their source URL, not the
        // version declared in their package.json.
        Some((Protocol::Http, _)) => candidate
            .manifest
            .dist()
            .is_some_and(|dist| dist.tarball.as_deref() == Some(spec)),
        _ => matches(spec, &candidate.version),
    }
}

/// Check a concrete requirement without performing I/O or selecting overrides.
/// Tags and non-registry sources need the final manifest to establish identity.
pub(crate) fn match_target(candidate: &PackageNode, name: &str, target: &str) -> MatchResult {
    match Protocol::strip_prefix(target) {
        Some((Protocol::Http, _)) => matches_spec(candidate, target).into(),
        None if !target.is_registry_spec() => MatchResult::NeedsResolution,
        None | Some((Protocol::NpmAlias, _)) => {
            let (target_name, target_range) = normalize_spec(name, target);
            if candidate.manifest.name() != target_name {
                return MatchResult::NoMatch;
            }
            match VersionReq::parse_from_npm(&target_range) {
                Ok(req) if req.tag().is_some() => MatchResult::NeedsResolution,
                Ok(_) => matches(&target_range, &candidate.version).into(),
                Err(_) => MatchResult::NoMatch,
            }
        }
        _ => MatchResult::NeedsResolution,
    }
}

/// Compare the final manifest after resolution, including any override.
/// The original request and override rules must not be applied again here.
pub(crate) fn matches_resolved_manifest(
    candidate: &PackageNode,
    manifest: &CoreVersionManifest,
) -> bool {
    candidate.manifest.name() == manifest.name
        && candidate.version == manifest.version
        && candidate
            .manifest
            .dist()
            .is_some_and(|dist| dist.tarball == manifest.dist.tarball)
}
