use deno_semver::{Version, VersionReq};

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
}
