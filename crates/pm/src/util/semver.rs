use deno_semver::{Version, VersionReq};
use tokio::time::Instant;

use crate::util::logger::log_verbose;

// check npm version, with npm: prefix & tag logic
pub fn matches(range: &str, version: &str) -> bool {
    let range = if range.starts_with("npm:") {
        if let Some(idx) = range.rfind('@') {
            &range[idx + 1..]
        } else {
            "*"
        }
    } else {
        range
    };

    if range == "*" {
        return true;
    }

    let req = match VersionReq::parse_from_npm(range) {
        Ok(req) => req,
        Err(_) => return false,
    };

    if req.tag().is_some() {
        return true;
    }

    let version = match Version::parse_from_npm(version) {
        Ok(v) => v,
        Err(_) => return false,
    };

    req.matches(&version)
}

pub fn is_valid_version(version: &str) -> bool {
    Version::parse_from_npm(version).is_ok()
}

/// Find the maximum version from a list of versions that satisfies the given range
pub fn max_satisfying<'a>(versions: impl Iterator<Item = &'a str>, range: &str) -> Option<Version> {
    let req = VersionReq::parse_from_npm(range).ok()?;
    let start = Instant::now();
    let res = versions
        .filter_map(|v| Version::parse_from_npm(v).ok())
        .filter(|v| req.matches(v))
        .max();
    log_verbose(&format!("max_satisfying took {:?}", start.elapsed()));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_matching() {
        assert!(matches("1.2.3", "1.2.3"));
        assert!(!matches("1.2.3", "1.2.4"));

        assert!(matches("^1.2.3", "1.3.0"));
        assert!(!matches("^1.2.3", "2.0.0"));

        assert!(matches("~1.2.3", "1.2.9"));
        assert!(!matches("~1.2.3", "1.3.0"));

        assert!(matches("*", "1.2.3"));

        assert!(matches("beta", "1.2.3"));

        assert!(!matches("1.2.3", "invalid"));
    }

    #[test]
    fn test_max_satisfying() {
        let versions = ["1.0.0", "1.1.0", "1.2.0", "2.0.0", "2.1.0"];

        assert_eq!(
            max_satisfying(versions.iter().cloned(), "^1.0.0"),
            Some(Version::parse_from_npm("1.2.0").unwrap())
        );

        assert_eq!(
            max_satisfying(versions.iter().cloned(), "^2.0.0"),
            Some(Version::parse_from_npm("2.1.0").unwrap())
        );

        assert_eq!(
            max_satisfying(versions.iter().cloned(), "~1.1.0"),
            Some(Version::parse_from_npm("1.1.0").unwrap())
        );

        assert_eq!(
            max_satisfying(versions.iter().cloned(), ">=2.0.0"),
            Some(Version::parse_from_npm("2.1.0").unwrap())
        );

        assert_eq!(max_satisfying(versions.iter().cloned(), "^3.0.0"), None);
    }
}
