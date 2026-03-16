//! Platform compatibility checking for package os/cpu constraints.
//!
//! Checks if a package's `os` and `cpu` fields are compatible with the current platform.

use serde_json::Value;
use std::env::consts::{ARCH, OS};

fn normalize_os(os: &str) -> String {
    match os.to_lowercase().as_str() {
        "darwin" | "macos" => "macos".to_string(),
        "win32" | "windows" => "win32".to_string(),
        other => other.to_string(),
    }
}

/// Check if the package's `os` constraint is compatible with the current platform.
///
/// The `os` field can be:
/// - A string: matches if equal to current OS
/// - An array: matches if any element matches (supports `!` prefix for negation)
/// - Other: always compatible (ignore invalid format)
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use utoo_ruborist::compat::is_os_compatible;
///
/// // String constraint
/// assert!(is_os_compatible(&json!("darwin"))); // on macOS
///
/// // Array with negation
/// assert!(is_os_compatible(&json!(["!win32"]))); // not Windows
/// ```
pub fn is_os_compatible(constraint: &Value) -> bool {
    match constraint {
        Value::String(os_name) => normalize_os(os_name) == normalize_os(OS),
        Value::Array(os_list) => {
            // Check for negations first - if any negation matches current OS, it's incompatible
            let current_os = normalize_os(OS);
            let has_negations = os_list
                .iter()
                .any(|os| os.as_str().is_some_and(|s| s.starts_with('!')));

            if has_negations {
                // With negations: compatible if no negation excludes current OS
                !os_list.iter().any(|os| {
                    os.as_str().is_some_and(|os_str| {
                        os_str.starts_with('!') && normalize_os(&os_str[1..]) == current_os
                    })
                })
            } else {
                // Without negations: compatible if any OS matches
                os_list.iter().any(|os| {
                    os.as_str()
                        .is_some_and(|os_str| normalize_os(os_str) == current_os)
                })
            }
        }
        _ => true, // ignore invalid format
    }
}

/// Check if the package's `cpu` constraint is compatible with the current architecture.
///
/// The `cpu` field can be:
/// - A string: matches if equal to current arch (with alias support)
/// - An array: matches if any element matches (supports `!` prefix for negation)
/// - Other: always compatible (ignore invalid format)
///
/// Supported aliases:
/// - `arm64` <-> `aarch64`
/// - `x64` <-> `x86_64`
pub fn is_cpu_compatible(constraint: &Value) -> bool {
    match constraint {
        Value::String(cpu_name) => {
            let cpu_name = cpu_name.to_lowercase();
            let arch = cpu_name.strip_prefix('!').unwrap_or(&cpu_name);
            let matches = is_arch_match(arch);
            if cpu_name.starts_with('!') {
                !matches
            } else {
                matches
            }
        }
        Value::Array(cpu_list) => {
            let has_negations = cpu_list
                .iter()
                .any(|cpu| cpu.as_str().is_some_and(|s| s.starts_with('!')));

            if has_negations {
                // With negations: compatible if no negation excludes current arch
                !cpu_list.iter().any(|cpu| {
                    cpu.as_str().is_some_and(|cpu_str| {
                        let cpu_str = cpu_str.to_lowercase();
                        cpu_str.starts_with('!') && is_arch_match(&cpu_str[1..])
                    })
                })
            } else {
                // Without negations: compatible if any arch matches
                cpu_list.iter().any(|cpu| {
                    cpu.as_str()
                        .is_some_and(|cpu_str| is_arch_match(&cpu_str.to_lowercase()))
                })
            }
        }
        _ => true, // ignore invalid format
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

/// Check if a package manifest is compatible with the current platform.
///
/// A package is compatible if both `os` and `cpu` constraints match (or are not specified).
pub fn is_platform_compatible(os: Option<&Value>, cpu: Option<&Value>) -> bool {
    let os_ok = os.is_none_or(is_os_compatible);
    let cpu_ok = cpu.is_none_or(is_cpu_compatible);
    os_ok && cpu_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            assert!(is_os_compatible(&json!("darwin")));
            assert!(is_os_compatible(&json!("macos")));
            assert!(!is_os_compatible(&json!("linux")));
            assert!(!is_os_compatible(&json!("win32")));
        }
        #[cfg(target_os = "linux")]
        {
            assert!(is_os_compatible(&json!("linux")));
            assert!(!is_os_compatible(&json!("darwin")));
        }
    }

    #[test]
    fn test_is_os_compatible_array() {
        #[cfg(target_os = "macos")]
        {
            assert!(is_os_compatible(&json!(["darwin", "linux"])));
            assert!(is_os_compatible(&json!(["linux", "darwin"])));
            assert!(!is_os_compatible(&json!(["linux", "win32"])));
        }
    }

    #[test]
    fn test_is_os_compatible_negation() {
        #[cfg(target_os = "macos")]
        {
            // Negation that excludes current OS
            assert!(!is_os_compatible(&json!(["!darwin"])));
            // Negation that doesn't exclude current OS
            assert!(is_os_compatible(&json!(["!linux"])));
            assert!(is_os_compatible(&json!(["!win32", "!linux"])));
        }
    }

    #[test]
    fn test_is_cpu_compatible() {
        // Test architecture aliases
        #[cfg(target_arch = "aarch64")]
        {
            assert!(is_cpu_compatible(&json!("arm64")));
            assert!(is_cpu_compatible(&json!("aarch64")));
            assert!(!is_cpu_compatible(&json!("x64")));
            assert!(is_cpu_compatible(&json!(["arm64", "x64"])));
        }
        #[cfg(target_arch = "x86_64")]
        {
            assert!(is_cpu_compatible(&json!("x64")));
            assert!(is_cpu_compatible(&json!("x86_64")));
            assert!(!is_cpu_compatible(&json!("arm64")));
        }
    }

    #[test]
    fn test_is_cpu_compatible_negation() {
        #[cfg(target_arch = "aarch64")]
        {
            assert!(!is_cpu_compatible(&json!(["!arm64"])));
            assert!(is_cpu_compatible(&json!(["!x64"])));
        }
    }

    #[test]
    fn test_is_platform_compatible() {
        // No constraints = compatible
        assert!(is_platform_compatible(None, None));

        // Only OS constraint
        #[cfg(target_os = "macos")]
        {
            assert!(is_platform_compatible(Some(&json!(["darwin"])), None));
            assert!(!is_platform_compatible(Some(&json!(["linux"])), None));
        }

        // Both constraints
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            assert!(is_platform_compatible(
                Some(&json!(["darwin"])),
                Some(&json!(["arm64"]))
            ));
            assert!(!is_platform_compatible(
                Some(&json!(["darwin"])),
                Some(&json!(["x64"]))
            ));
            assert!(!is_platform_compatible(
                Some(&json!(["linux"])),
                Some(&json!(["arm64"]))
            ));
        }
    }

    #[test]
    fn test_invalid_format() {
        // Invalid formats should return true (compatible)
        assert!(is_os_compatible(&json!(123)));
        assert!(is_os_compatible(&json!(null)));
        assert!(is_cpu_compatible(&json!(123)));
        assert!(is_cpu_compatible(&json!(null)));
    }
}
