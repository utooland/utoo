//! Platform compatibility checking for package os/cpu constraints.
//!
//! Checks if a package's `os` and `cpu` fields are compatible with the current platform.

use std::env::consts::{ARCH, OS};

use serde::{Deserialize, Serialize};

/// npm `os`/`cpu` constraint, as it appears on every wire (package.json,
/// registry version manifests, lockfiles).
///
/// The spec says array-of-strings with `!` negation; the npm CLI
/// (`npm-install-checks`' `checkList`) additionally tolerates a bare string,
/// and real packages ship both shapes. This untagged enum captures exactly
/// those two legal forms — anything else fails deserialization, which every
/// carrier field maps to `None` (= unconstrained), matching npm's behavior
/// for unusable values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlatformConstraint {
    /// Spec form: `"os": ["linux", "!win32"]`.
    Many(Vec<String>),
    /// Bare-string form npm tolerates: `"os": "linux"`.
    One(String),
}

impl PlatformConstraint {
    fn entries(&self) -> impl Iterator<Item = &str> {
        let slice = match self {
            Self::One(s) => std::slice::from_ref(s),
            Self::Many(v) => v.as_slice(),
        };
        slice.iter().map(String::as_str)
    }

    /// npm's `checkList` semantics: when any negation (`!x`) is present,
    /// negations veto — compatible iff no negation matches the platform.
    /// Otherwise at least one positive entry must match.
    fn allows(&self, matches: impl Fn(&str) -> bool) -> bool {
        let has_negations = self.entries().any(|e| e.starts_with('!'));
        if has_negations {
            !self
                .entries()
                .filter_map(|e| e.strip_prefix('!'))
                .any(&matches)
        } else {
            self.entries().any(&matches)
        }
    }
}

fn normalize_os(os: &str) -> String {
    match os.to_lowercase().as_str() {
        "darwin" | "macos" => "macos".to_string(),
        "win32" | "windows" => "win32".to_string(),
        other => other.to_string(),
    }
}

fn is_arch_match(arch: &str) -> bool {
    let current_arch = ARCH.to_lowercase();
    match arch {
        "arm64" | "aarch64" => current_arch == "aarch64",
        "x64" | "x86_64" => current_arch == "x86_64",
        _ => arch == current_arch,
    }
}

/// Check if the package's `os` constraint is compatible with the current platform.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use utoo_ruborist::compat::{PlatformConstraint, is_os_compatible};
///
/// // Bare-string form (npm-tolerated)
/// let c: PlatformConstraint = serde_json::from_value(json!("darwin")).unwrap();
/// # #[cfg(target_os = "macos")]
/// assert!(is_os_compatible(&c));
///
/// // Array with negation
/// let c: PlatformConstraint = serde_json::from_value(json!(["!win32"])).unwrap();
/// # #[cfg(not(target_os = "windows"))]
/// assert!(is_os_compatible(&c));
/// ```
pub fn is_os_compatible(constraint: &PlatformConstraint) -> bool {
    let current_os = normalize_os(OS);
    constraint.allows(|os| normalize_os(os) == current_os)
}

/// Check if the package's `cpu` constraint is compatible with the current
/// architecture.
///
/// Supported aliases: `arm64` ↔ `aarch64`, `x64` ↔ `x86_64`.
pub fn is_cpu_compatible(constraint: &PlatformConstraint) -> bool {
    constraint.allows(|cpu| is_arch_match(&cpu.to_lowercase()))
}

/// Check if a package manifest is compatible with the current platform.
///
/// A package is compatible if both `os` and `cpu` constraints match (or are not specified).
pub fn is_platform_compatible(
    os: Option<&PlatformConstraint>,
    cpu: Option<&PlatformConstraint>,
) -> bool {
    let os_ok = os.is_none_or(is_os_compatible);
    let cpu_ok = cpu.is_none_or(is_cpu_compatible);
    os_ok && cpu_ok
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn pc(v: serde_json::Value) -> PlatformConstraint {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn test_normalize_os() {
        assert_eq!(normalize_os("darwin"), "macos");
        assert_eq!(normalize_os("macos"), "macos");
        assert_eq!(normalize_os("linux"), "linux");
        assert_eq!(normalize_os("win32"), "win32");
    }

    #[test]
    fn test_is_os_compatible_string() {
        // Current OS should match
        #[cfg(target_os = "macos")]
        {
            assert!(is_os_compatible(&pc(json!("darwin"))));
            assert!(is_os_compatible(&pc(json!("macos"))));
            assert!(!is_os_compatible(&pc(json!("linux"))));
            assert!(!is_os_compatible(&pc(json!("win32"))));
            // String-form negation behaves like npm's coerced single-element
            // list (the old Value-based matcher missed this case for os).
            assert!(is_os_compatible(&pc(json!("!win32"))));
            assert!(!is_os_compatible(&pc(json!("!darwin"))));
        }
        #[cfg(target_os = "linux")]
        {
            assert!(is_os_compatible(&pc(json!("linux"))));
            assert!(!is_os_compatible(&pc(json!("darwin"))));
        }
    }

    #[test]
    fn test_is_os_compatible_array() {
        #[cfg(target_os = "macos")]
        {
            assert!(is_os_compatible(&pc(json!(["darwin", "linux"]))));
            assert!(is_os_compatible(&pc(json!(["linux", "darwin"]))));
            assert!(!is_os_compatible(&pc(json!(["linux", "win32"]))));
        }
    }

    #[test]
    fn test_is_os_compatible_negation() {
        #[cfg(target_os = "macos")]
        {
            // Negation that excludes current OS
            assert!(!is_os_compatible(&pc(json!(["!darwin"]))));
            // Negation that doesn't exclude current OS
            assert!(is_os_compatible(&pc(json!(["!linux"]))));
            assert!(is_os_compatible(&pc(json!(["!win32", "!linux"]))));
        }
    }

    #[test]
    fn test_is_cpu_compatible() {
        // Test architecture aliases
        #[cfg(target_arch = "aarch64")]
        {
            assert!(is_cpu_compatible(&pc(json!("arm64"))));
            assert!(is_cpu_compatible(&pc(json!("aarch64"))));
            assert!(!is_cpu_compatible(&pc(json!("x64"))));
            assert!(is_cpu_compatible(&pc(json!(["arm64", "x64"]))));
        }
        #[cfg(target_arch = "x86_64")]
        {
            assert!(is_cpu_compatible(&pc(json!("x64"))));
            assert!(is_cpu_compatible(&pc(json!("x86_64"))));
            assert!(!is_cpu_compatible(&pc(json!("arm64"))));
        }
    }

    #[test]
    fn test_is_cpu_compatible_negation() {
        #[cfg(target_arch = "aarch64")]
        {
            assert!(!is_cpu_compatible(&pc(json!(["!arm64"]))));
            assert!(is_cpu_compatible(&pc(json!(["!x64"]))));
        }
    }

    #[test]
    fn test_is_platform_compatible() {
        // No constraints = compatible
        assert!(is_platform_compatible(None, None));

        // Only OS constraint
        #[cfg(target_os = "macos")]
        {
            assert!(is_platform_compatible(Some(&pc(json!(["darwin"]))), None));
            assert!(!is_platform_compatible(Some(&pc(json!(["linux"]))), None));
        }

        // Both constraints
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            assert!(is_platform_compatible(
                Some(&pc(json!(["darwin"]))),
                Some(&pc(json!(["arm64"])))
            ));
            assert!(!is_platform_compatible(
                Some(&pc(json!(["darwin"]))),
                Some(&pc(json!(["x64"])))
            ));
            assert!(!is_platform_compatible(
                Some(&pc(json!(["linux"]))),
                Some(&pc(json!(["arm64"])))
            ));
        }
    }

    #[test]
    fn test_invalid_format_fails_deserialization() {
        // Non string/array shapes are not representable; carrier fields map
        // the deserialization failure to None (= unconstrained), preserving
        // the old "invalid format is compatible" behavior at one level up.
        assert!(serde_json::from_value::<PlatformConstraint>(json!(123)).is_err());
        assert!(serde_json::from_value::<PlatformConstraint>(json!(null)).is_err());
        assert!(serde_json::from_value::<PlatformConstraint>(json!({"a": 1})).is_err());
    }
}
