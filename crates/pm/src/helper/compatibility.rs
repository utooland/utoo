use serde_json::Value;
use std::env::consts::{ARCH, OS};

fn normalize_os(os: &str) -> String {
    match os.to_lowercase().as_str() {
        "darwin" | "macos" => "macos".to_string(),
        other => other.to_string(),
    }
}

pub fn is_os_compatible(constraint: &Value) -> bool {
    match constraint {
        Value::String(os_name) => normalize_os(os_name) == normalize_os(OS),
        Value::Array(os_list) => {
            let current_os = normalize_os(OS);
            os_list.iter().any(|os| {
                if let Some(os_str) = os.as_str() {
                    let matches =
                        normalize_os(os_str.strip_prefix('!').unwrap_or(os_str)) == current_os;
                    if os_str.starts_with('!') {
                        !matches
                    } else {
                        matches
                    }
                } else {
                    false
                }
            })
        }
        _ => true, // ignore when format is incorrect
    }
}

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
        Value::Array(cpu_list) => cpu_list.iter().any(|cpu| {
            cpu.as_str().is_some_and(|cpu_str| {
                let cpu_str = cpu_str.to_lowercase();
                let arch = cpu_str.strip_prefix('!').unwrap_or(&cpu_str);
                let matches = is_arch_match(arch);
                if cpu_str.starts_with('!') {
                    !matches
                } else {
                    matches
                }
            })
        }),
        _ => true, // ignore when format is incorrect
    }
}

fn is_arch_match(arch: &str) -> bool {
    let current_arch = ARCH.to_lowercase();
    match arch {
        "arm64" => current_arch == "aarch64",
        "aarch64" => current_arch == "aarch64",
        "x64" => current_arch == "x86_64",
        _ => arch == current_arch,
    }
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
    }

    #[test]
    fn test_is_cpu_compatible() {
        // Test string value
        assert!(is_cpu_compatible(&json!(ARCH)));
        assert!(!is_cpu_compatible(&json!("invalid_arch")));

        // Test array value
        assert!(is_cpu_compatible(&json!([ARCH])));
        assert!(is_cpu_compatible(&json!(["invalid_arch", ARCH])));
        assert!(!is_cpu_compatible(&json!([
            "invalid_arch",
            "another_invalid"
        ])));

        // Test negation
        assert!(!is_cpu_compatible(&json!([format!("!{}", ARCH)])));
        assert!(is_cpu_compatible(&json!(["!invalid_arch"])));

        // Test architecture aliases
        if ARCH == "aarch64" {
            assert!(is_cpu_compatible(&json!("arm64")));
            assert!(is_cpu_compatible(&json!("aarch64")));
        } else if ARCH == "x86_64" {
            assert!(is_cpu_compatible(&json!("x64")));
        }

        // Test invalid format
        assert!(is_cpu_compatible(&json!(123)));
    }
}
