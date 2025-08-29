pub fn parse_package_name(path: &str) -> (Option<String>, String, String) {
    let parts: Vec<&str> = path.split('/').collect();
    let len = parts.len();

    if len >= 2 {
        let last = parts[len - 1];
        let second_last = parts[len - 2];

        if second_last.starts_with('@') {
            // scoped package: @scope/name
            (
                Some(second_last.to_string()),
                last.to_string(),
                format!("{second_last}/{last}"),
            )
        } else {
            // normal package
            (None, last.to_string(), last.to_string())
        }
    } else if len == 1 {
        // name only
        (None, parts[0].to_string(), parts[0].to_string())
    } else {
        // invalid path
        (None, path.to_string(), path.to_string())
    }
}

// Parse a package name with version specification into (name, version) tuple.
//
// # Examples
// ```
// let (name, version) = parse_package_spec("@a/b@1.0.0");
// assert_eq!(name, "@a/b");
// assert_eq!(version, "1.0.0");
//
// let (name, version) = parse_package_spec("lodash@^4.17.20");
// assert_eq!(name, "lodash");
// assert_eq!(version, "^4.17.20");
// ```
pub fn parse_package_spec(spec: &str) -> (&str, &str) {
    if let Some(stripped) = spec.strip_prefix('@') {
        if let Some(idx) = stripped.find('@') {
            let idx = idx + 1;
            (&spec[..idx], &spec[idx + 1..])
        } else {
            (spec, "*")
        }
    } else {
        spec.rfind('@')
            .map_or((spec, "*"), |pos| (&spec[..pos], &spec[pos + 1..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_spec() {
        // Test scoped packages
        let (name, version) = parse_package_spec("@a/b@1.0.0");
        assert_eq!(name, "@a/b");
        assert_eq!(version, "1.0.0");

        let (name, version) = parse_package_spec("@scope/pkg@^2.0.0");
        assert_eq!(name, "@scope/pkg");
        assert_eq!(version, "^2.0.0");

        let (name, version) = parse_package_spec("@a/b");
        assert_eq!(name, "@a/b");
        assert_eq!(version, "*");

        // Test regular packages
        let (name, version) = parse_package_spec("lodash@4.17.20");
        assert_eq!(name, "lodash");
        assert_eq!(version, "4.17.20");

        let (name, version) = parse_package_spec("express@^4.17.1");
        assert_eq!(name, "express");
        assert_eq!(version, "^4.17.1");

        let (name, version) = parse_package_spec("react");
        assert_eq!(name, "react");
        assert_eq!(version, "*");
    }
}
